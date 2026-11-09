#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "macos"))]
compile_error!("Chatly is a macOS app.");

mod biometric;
mod guard;
mod netcheck;
#[cfg(debug_assertions)]
mod selftest;
mod services;
mod vault;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering::SeqCst},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    webview::{DownloadEvent, NewWindowResponse},
    AppHandle, LogicalPosition, LogicalSize, Manager, Rect, RunEvent, TitleBarStyle, Url, WebviewBuilder, WebviewUrl,
    Window, WindowBuilder, WindowEvent, Wry,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use zeroize::Zeroizing;

use services::Service;
use vault::{Key, Settings, StoredCookie, Vault};

const MAIN: &str = "main";
const SHELL: &str = "shell";
const RAIL_WIDTH: f64 = 76.0;
const MIN_PASSWORD_CHARS: usize = 8;
const WIPE_AFTER: u32 = 10;
const SAVE_EVERY: Duration = Duration::from_secs(10 * 60);
const SLEEP_GAP: Duration = Duration::from_secs(20);
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                          (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const CONTENT_SCRIPT: &str = include_str!("content.js");
const PAUSE_MEDIA: &str = "document.querySelectorAll('video,audio').forEach(function(m){try{m.pause()}catch(e){}})";

type Res<T> = Result<T, String>;

struct Inner {
    vault: Option<Vault>,
    locked: bool,
    overlay: bool,
    active: Service,
    open: HashSet<Service>,
}

struct Chatly {
    vault_path: PathBuf,
    inner: Mutex<Inner>,
    key: Mutex<Option<Key>>,
    last_active: Mutex<SystemTime>,
    sessions: Mutex<()>,
    modal: AtomicBool,
    exiting: AtomicBool,
}

fn relock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Chatly {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        relock(&self.inner)
    }

    fn key(&self) -> MutexGuard<'_, Option<Key>> {
        relock(&self.key)
    }

    fn persist(&self, inner: &Inner) {
        if let Some(v) = &inner.vault {
            if let Err(e) = v.save(&self.vault_path) {
                eprintln!("chatly: couldn't save the vault: {e}");
            }
        }
    }

    fn touch(&self) {
        *relock(&self.last_active) = SystemTime::now();
    }
}

fn state(app: &AppHandle) -> Arc<Chatly> {
    app.state::<Arc<Chatly>>().inner().clone()
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn locked_error() -> String {
    "Chatly is locked.".into()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    phase: &'static str,
    active: Service,
    overlay: bool,
    settings: Settings,
    touch_id_available: bool,
    touch_id_enabled: bool,
    lockout_secs: u64,
    attempts_left: Option<u32>,
}

fn status_of(app: &AppHandle) -> Status {
    let touch_id_available = biometric::available();
    let st = state(app);
    let i = st.inner();
    let phase = match (&i.vault, i.locked) {
        (None, _) => "setup",
        (Some(_), true) => "locked",
        (Some(_), false) => "unlocked",
    };
    let v = i.vault.as_ref();
    Status {
        phase,
        active: i.active,
        overlay: i.overlay && phase == "unlocked",
        settings: v.map(|v| v.settings.clone()).unwrap_or_default(),
        touch_id_available,
        touch_id_enabled: v.is_some_and(|v| v.touch_id),
        lockout_secs: v.map_or(0, |v| v.lockout_remaining(unix_now())),
        attempts_left: v
            .filter(|v| v.settings.wipe_after_failures)
            .map(|v| WIPE_AFTER.saturating_sub(v.failed_attempts)),
    }
}

fn call_shell(app: &AppHandle, call: &str) {
    if let Some(shell) = app.get_webview(SHELL) {
        let _ = shell.eval(format!("window.chatly&&window.chatly.{call}"));
    }
}

fn push_state(app: &AppHandle) {
    let json = serde_json::to_string(&status_of(app)).unwrap_or_else(|_| "null".into());
    call_shell(app, &format!("update({json})"));
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { position: LogicalPosition::new(x, y).into(), size: LogicalSize::new(w.max(0.0), h.max(0.0)).into() }
}

fn layout(app: &AppHandle) {
    let Some(win) = app.get_window(MAIN) else { return };
    let (Ok(size), Ok(scale)) = (win.inner_size(), win.scale_factor()) else { return };
    let size: LogicalSize<f64> = size.to_logical(scale);
    let (full, active, open) = {
        let st = state(app);
        let i = st.inner();
        (i.vault.is_none() || i.locked || i.overlay, i.active, i.open.clone())
    };
    for svc in &open {
        if full || *svc != active {
            if let Some(wv) = app.get_webview(svc.label()) {
                let _ = wv.hide();
            }
        }
    }
    if let Some(shell) = app.get_webview(SHELL) {
        let width = if full { size.width } else { RAIL_WIDTH };
        let _ = shell.set_bounds(rect(0.0, 0.0, width, size.height));
    }
    if !full {
        if let Some(wv) = app.get_webview(active.label()) {
            let _ = wv.set_bounds(rect(RAIL_WIDTH, 0.0, size.width - RAIL_WIDTH, size.height));
            let _ = wv.show();
        }
    }
}

fn unread_count(title: &str) -> u32 {
    title
        .trim_start()
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
        .and_then(|(n, _)| n.trim().trim_end_matches('+').parse().ok())
        .unwrap_or(0)
}

fn content_webview(app: &AppHandle, svc: Service) -> WebviewBuilder<Wry> {
    let for_popups = app.clone();
    let for_badges = app.clone();
    let for_downloads = app.clone();
    WebviewBuilder::new(svc.label(), WebviewUrl::External(Url::parse("about:blank").expect("static URL")))
        .incognito(true)
        .user_agent(USER_AGENT)
        .initialization_script(CONTENT_SCRIPT)
        .disable_drag_drop_handler()
        .on_navigation(|url| {
            let allowed = services::allowed_in_app(url);
            #[cfg(debug_assertions)]
            if !allowed {
                eprintln!("chatly: blocked a frame navigation to {url}");
            }
            allowed
        })
        .on_new_window(move |url, _| {
            open_popup(&for_popups, svc, url);
            NewWindowResponse::Deny
        })
        .on_document_title_changed(move |_, title| {
            #[cfg(debug_assertions)]
            if let Some(probe) = title.strip_prefix("E2E:") {
                *relock(&selftest::TITLE) = probe.to_string();
            }
            call_shell(&for_badges, &format!("badge({:?},{})", svc.id(), unread_count(&title)));
        })
        .on_download(move |_, event| match event {
            DownloadEvent::Requested { .. } => !state(&for_downloads).inner().locked,
            _ => true,
        })
}

fn ensure_webview(app: &AppHandle, svc: Service) -> Res<()> {
    let st = state(app);
    let _sessions = relock(&st.sessions);
    if app.get_webview(svc.label()).is_some() {
        return Ok(());
    }
    let key = st.key().clone();
    let cookies = {
        let i = st.inner();
        match (key, &i.vault) {
            (Some(key), Some(v)) => v
                .read_sessions(&key)
                .ok()
                .and_then(|mut s| s.remove(svc.id()))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    };
    let win = app.get_window(MAIN).ok_or("Chatly's window is gone.")?;
    let wv = win
        .add_child(content_webview(app, svc), LogicalPosition::new(RAIL_WIDTH, 0.0), LogicalSize::new(1.0, 1.0))
        .map_err(|e| e.to_string())?;
    let _ = wv.hide();
    let now = unix_now() as i64;
    let restored = cookies
        .iter()
        .filter(|c| !c.is_expired(now))
        .filter(|c| wv.set_cookie(c.to_cookie()).is_ok())
        .count();
    #[cfg(debug_assertions)]
    eprintln!("chatly: {}: restored {restored} of {} cookies", svc.id(), cookies.len());
    let _ = restored;
    wv.navigate(svc.home()).map_err(|e| e.to_string())?;
    st.inner().open.insert(svc);
    Ok(())
}

fn save_sessions(app: &AppHandle) {
    let st = state(app);
    let _sessions = relock(&st.sessions);
    let Some(key) = st.key().clone() else { return };
    let (open, remember) = {
        let i = st.inner();
        let Some(v) = &i.vault else { return };
        (i.open.clone(), v.settings.remember_sessions)
    };
    let mut fresh = Vec::new();
    if remember {
        for svc in open {
            let Some(wv) = app.get_webview(svc.label()) else { continue };
            match wv.cookies() {
                Ok(cookies) => {
                    let kept: Vec<_> = cookies.iter().filter_map(StoredCookie::from_cookie).collect();
                    fresh.push((svc, kept));
                }
                Err(e) => eprintln!("chatly: couldn't read {} cookies: {e}", svc.id()),
            }
        }
    }
    let mut i = st.inner();
    let Some(v) = i.vault.as_mut() else { return };
    if remember {
        let mut sessions = v.read_sessions(&key).unwrap_or_default();
        for (svc, cookies) in fresh {
            #[cfg(debug_assertions)]
            eprintln!("chatly: {}: saved {} cookies", svc.id(), cookies.len());
            sessions.insert(svc.id().to_string(), cookies);
        }
        if let Err(e) = v.write_sessions(&key, &sessions) {
            eprintln!("chatly: couldn't encrypt sessions: {e}");
            return;
        }
    } else {
        v.sessions = None;
    }
    st.persist(&i);
}

fn lock_now(app: &AppHandle) {
    let st = state(app);
    let open = {
        let mut i = st.inner();
        if i.vault.is_none() || i.locked {
            return;
        }
        i.locked = true;
        i.overlay = false;
        i.open.clone()
    };
    for svc in &open {
        if let Some(wv) = app.get_webview(svc.label()) {
            let _ = wv.eval(PAUSE_MEDIA);
        }
    }
    layout(app);
    push_state(app);
    if let Some(shell) = app.get_webview(SHELL) {
        let _ = shell.set_focus();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        save_sessions(&app);
        let st = state(&app);
        let mut key = st.key();
        if st.inner().locked {
            *key = None;
        }
    });
}

fn finish_unlock(app: &AppHandle, key: Key) -> Res<Status> {
    let st = state(app);
    {
        let mut k = st.key();
        let mut i = st.inner();
        *k = Some(key);
        i.locked = false;
        i.overlay = false;
        if let Some(v) = i.vault.as_mut() {
            v.record_success();
        }
        st.persist(&i);
    }
    st.touch();
    let active = st.inner().active;
    ensure_webview(app, active)?;
    layout(app);
    push_state(app);
    if let Some(wv) = app.get_webview(active.label()) {
        let _ = wv.set_focus();
    }
    Ok(status_of(app))
}

fn human_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} seconds"),
        60..=119 => "1 minute".into(),
        _ => format!("{} minutes", secs.div_ceil(60)),
    }
}

fn failed_attempt(app: &AppHandle, now: u64) -> String {
    let st = state(app);
    let (failures, wipe, wait) = {
        let mut i = st.inner();
        let Some(v) = i.vault.as_mut() else { return "Wrong password.".into() };
        v.record_failure(now);
        let result = (v.failed_attempts, v.settings.wipe_after_failures, v.lockout_remaining(now));
        st.persist(&i);
        result
    };
    if wipe && failures >= WIPE_AFTER {
        erase_everything_now(app);
        return format!("{WIPE_AFTER} wrong passwords in a row. Chatly erased its saved sessions and password.");
    }
    let mut msg = String::from("Wrong password.");
    if wipe && WIPE_AFTER - failures <= 5 {
        msg += &format!(" {} more and Chatly erases everything.", WIPE_AFTER - failures);
    }
    if wait > 0 {
        msg += &format!(" Try again in {}.", human_duration(wait));
    }
    msg
}

fn disable_touch_id(app: &AppHandle) {
    biometric::forget();
    let st = state(app);
    {
        let mut i = st.inner();
        if let Some(v) = i.vault.as_mut() {
            v.touch_id = false;
        }
        st.persist(&i);
    }
    push_state(app);
}

fn erase_everything_now(app: &AppHandle) {
    let st = state(app);
    let _sessions = relock(&st.sessions);
    let open: Vec<Service> = st.inner().open.drain().collect();
    for svc in open {
        if let Some(wv) = app.get_webview(svc.label()) {
            let _ = wv.close();
        }
    }
    biometric::forget();
    if let Err(e) = std::fs::remove_file(&st.vault_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("chatly: couldn't delete the vault: {e}");
        }
    }
    {
        let mut k = st.key();
        let mut i = st.inner();
        *k = None;
        i.vault = None;
        i.locked = true;
        i.overlay = false;
    }
    layout(app);
    push_state(app);
}

fn quit(app: &AppHandle) {
    let st = state(app);
    if st.exiting.swap(true, SeqCst) {
        return;
    }
    let open = st.inner().open.clone();
    for svc in open {
        if let Some(wv) = app.get_webview(svc.label()) {
            let _ = wv.hide();
        }
    }
    let app = app.clone();
    std::thread::spawn(move || {
        save_sessions(&app);
        app.exit(0);
    });
}

fn open_popup(app: &AppHandle, svc: Service, url: Url) {
    if state(app).inner().locked {
        return;
    }
    if let Some(target) = services::unwrap_link_shim(&url) {
        return open_in_browser(app, target);
    }
    if services::is_meta(&url) {
        let app = app.clone();
        std::thread::spawn(move || {
            if let Some(wv) = app.get_webview(svc.label()) {
                let _ = wv.navigate(url);
            }
        });
        return;
    }
    if matches!(url.scheme(), "http" | "https") {
        open_in_browser(app, url);
    }
}

fn open_in_browser(app: &AppHandle, url: Url) {
    let url = services::strip_tracking(url);
    let st = state(app);
    let confirm = st.inner().vault.as_ref().is_none_or(|v| v.settings.confirm_links);
    if !confirm {
        return launch_browser(&url);
    }
    let Some(win) = app.get_window(MAIN) else { return };
    let mut shown: String = url.as_str().chars().take(300).collect();
    if shown.len() < url.as_str().len() {
        shown.push('…');
    }
    st.modal.store(true, SeqCst);
    let app = app.clone();
    app.dialog()
        .message(format!("{shown}\n\nIt opens in your default browser, outside Chatly's lock."))
        .title("Open this link in your browser?")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom("Open in Browser".into(), "Cancel".into()))
        .parent(&win)
        .show({
            let app = app.clone();
            move |open| {
                let st = state(&app);
                st.modal.store(false, SeqCst);
                st.touch();
                if open && !st.inner().locked {
                    launch_browser(&url);
                }
            }
        });
}

fn launch_browser(url: &Url) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    if let Ok(mut child) = std::process::Command::new("/usr/bin/open").arg(url.as_str()).spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn navigate_active(app: &AppHandle, action: &str) {
    let st = state(app);
    let active = {
        let i = st.inner();
        if i.vault.is_none() || i.locked {
            return;
        }
        i.active
    };
    if matches!(action, "home" | "messages") {
        let _ = set_overlay_now(app, false);
    }
    let Some(wv) = app.get_webview(active.label()) else { return };
    let _ = match action {
        "back" => wv.eval("history.back()"),
        "forward" => wv.eval("history.forward()"),
        "reload" => wv.reload(),
        "home" => wv.navigate(active.home()),
        "messages" => wv.navigate(active.messages()),
        _ => Ok(()),
    };
}

fn select(app: &AppHandle, svc: Service) -> Res<()> {
    let st = state(app);
    {
        let mut i = st.inner();
        if i.vault.is_none() || i.locked {
            return Err(locked_error());
        }
        i.active = svc;
        i.overlay = false;
        if let Some(v) = i.vault.as_mut() {
            v.last_service = svc;
        }
        st.persist(&i);
    }
    ensure_webview(app, svc)?;
    layout(app);
    push_state(app);
    if let Some(wv) = app.get_webview(svc.label()) {
        let _ = wv.set_focus();
    }
    Ok(())
}

fn set_overlay_now(app: &AppHandle, open: bool) -> Res<()> {
    let st = state(app);
    let active = {
        let mut i = st.inner();
        if i.vault.is_none() || i.locked {
            return Err(locked_error());
        }
        i.overlay = open;
        i.active
    };
    layout(app);
    push_state(app);
    if !open {
        if let Some(wv) = app.get_webview(active.label()) {
            let _ = wv.set_focus();
        }
    }
    Ok(())
}

fn require_unlocked(app: &AppHandle) -> Res<Arc<Chatly>> {
    let st = state(app);
    let unlocked = {
        let i = st.inner();
        i.vault.is_some() && !i.locked
    };
    if unlocked {
        Ok(st)
    } else {
        Err(locked_error())
    }
}

fn check_new_password(pw: &str) -> Res<()> {
    match pw.chars().count() {
        n if n < MIN_PASSWORD_CHARS => Err(format!("Use at least {MIN_PASSWORD_CHARS} characters.")),
        n if n > 1024 => Err("That password is too long.".into()),
        _ => Ok(()),
    }
}

#[tauri::command]
fn status(app: AppHandle) -> Status {
    status_of(&app)
}

#[tauri::command]
async fn setup(app: AppHandle, password: String) -> Res<Status> {
    let password = Zeroizing::new(password);
    check_new_password(&password)?;
    let st = state(&app);
    if st.inner().vault.is_some() {
        return Err("Chatly already has a password.".into());
    }
    let (vault, key) = blocking(move || Vault::create(&password)).await?.map_err(|e| e.to_string())?;
    vault.save(&st.vault_path).map_err(|e| e.to_string())?;
    biometric::forget();
    st.inner().vault = Some(vault);
    finish_unlock(&app, key)
}

#[tauri::command]
async fn unlock(app: AppHandle, password: String) -> Res<Status> {
    let password = Zeroizing::new(password);
    let st = state(&app);
    let now = unix_now();
    let vault = {
        let i = st.inner();
        match &i.vault {
            None => return Err("Set a password first.".into()),
            Some(_) if !i.locked => None,
            Some(v) => {
                let wait = v.lockout_remaining(now);
                if wait > 0 {
                    return Err(format!("Too many attempts. Try again in {}.", human_duration(wait)));
                }
                Some(v.clone())
            }
        }
    };
    let Some(vault) = vault else { return Ok(status_of(&app)) };
    match blocking(move || vault.unlock(&password)).await? {
        Ok(key) => finish_unlock(&app, key),
        Err(vault::Error::WrongPassword) => Err(failed_attempt(&app, now)),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn unlock_touch_id(app: AppHandle) -> Res<Status> {
    let st = state(&app);
    let (locked, enabled) = {
        let i = st.inner();
        (i.locked, i.vault.as_ref().is_some_and(|v| v.touch_id))
    };
    if !locked {
        return Ok(status_of(&app));
    }
    if !enabled {
        return Err("Touch ID is turned off.".into());
    }
    st.modal.store(true, SeqCst);
    let result = blocking(biometric::unlock).await;
    st.modal.store(false, SeqCst);
    let key = match result? {
        Ok(key) => key,
        Err(biometric::UnlockError::Prompt(msg)) => return Err(msg),
        Err(biometric::UnlockError::Invalidated) => {
            disable_touch_id(&app);
            return Err("Your fingerprints changed, so Touch ID was turned off. Unlock with your password.".into());
        }
    };
    if !st.inner().vault.as_ref().is_some_and(|v| v.check_key(&key)) {
        disable_touch_id(&app);
        return Err("The saved Touch ID key doesn't fit this vault. Unlock with your password.".into());
    }
    finish_unlock(&app, key)
}

#[tauri::command]
fn lock(app: AppHandle) {
    lock_now(&app);
}

#[tauri::command]
async fn select_service(app: AppHandle, service: Service) -> Res<Status> {
    select(&app, service)?;
    Ok(status_of(&app))
}

#[tauri::command]
async fn set_overlay(app: AppHandle, open: bool) -> Res<Status> {
    set_overlay_now(&app, open)?;
    Ok(status_of(&app))
}

#[tauri::command]
async fn update_settings(app: AppHandle, settings: Settings) -> Res<Status> {
    let st = require_unlocked(&app)?;
    let settings = settings.sanitized();
    let was_remembering = {
        let mut i = st.inner();
        let v = i.vault.as_mut().ok_or_else(locked_error)?;
        let was = v.settings.remember_sessions;
        v.settings = settings.clone();
        if !settings.remember_sessions {
            v.sessions = None;
        }
        st.persist(&i);
        was
    };
    if let Some(win) = app.get_window(MAIN) {
        let _ = win.set_content_protected(settings.hide_from_capture);
    }
    if settings.remember_sessions && !was_remembering {
        save_sessions(&app);
    }
    Ok(status_of(&app))
}

#[tauri::command]
async fn change_password(app: AppHandle, current: String, next: String) -> Res<()> {
    let current = Zeroizing::new(current);
    let next = Zeroizing::new(next);
    check_new_password(&next)?;
    let st = require_unlocked(&app)?;
    let key = st.key().clone().ok_or_else(locked_error)?;
    let vault = st.inner().vault.clone().ok_or_else(locked_error)?;
    let updated = blocking(move || -> Result<Vault, vault::Error> {
        let mut vault = vault;
        vault.unlock(&current)?;
        vault.rewrap(&key, &next)?;
        Ok(vault)
    })
    .await?
    .map_err(|e| match e {
        vault::Error::WrongPassword => "Your current password is wrong.".to_string(),
        e => e.to_string(),
    })?;
    let mut i = st.inner();
    if let Some(v) = i.vault.as_mut() {
        v.kdf = updated.kdf;
        v.wrapped_key = updated.wrapped_key;
    }
    st.persist(&i);
    Ok(())
}

#[tauri::command]
async fn set_touch_id(app: AppHandle, enabled: bool) -> Res<Status> {
    let st = require_unlocked(&app)?;
    if !enabled {
        disable_touch_id(&app);
        return Ok(status_of(&app));
    }
    if !biometric::available() {
        return Err("Touch ID isn't available on this Mac right now.".into());
    }
    let key = st.key().clone().ok_or_else(locked_error)?;
    st.modal.store(true, SeqCst);
    let result = blocking(move || biometric::enroll(&key)).await;
    st.modal.store(false, SeqCst);
    st.touch();
    result??;
    {
        let mut i = st.inner();
        if let Some(v) = i.vault.as_mut() {
            v.touch_id = true;
        }
        st.persist(&i);
    }
    Ok(status_of(&app))
}

#[tauri::command]
async fn sign_out(app: AppHandle, service: Service) -> Res<Status> {
    let st = require_unlocked(&app)?;
    {
        let _sessions = relock(&st.sessions);
        if let Some(wv) = app.get_webview(service.label()) {
            let _ = wv.close();
        }
        let key = st.key().clone();
        let mut i = st.inner();
        i.open.remove(&service);
        if let (Some(key), Some(v)) = (key, i.vault.as_mut()) {
            let mut sessions = v.read_sessions(&key).unwrap_or_default();
            sessions.remove(service.id());
            if let Err(e) = v.write_sessions(&key, &sessions) {
                return Err(e.to_string());
            }
        }
        st.persist(&i);
    }
    let active = st.inner().active;
    if active == service {
        ensure_webview(&app, service)?;
        layout(&app);
    }
    Ok(status_of(&app))
}

#[tauri::command]
async fn erase_everything(app: AppHandle) -> Res<Status> {
    erase_everything_now(&app);
    Ok(status_of(&app))
}

#[tauri::command]
async fn security_check(app: AppHandle) -> Res<Vec<netcheck::Finding>> {
    require_unlocked(&app)?;
    blocking(netcheck::run).await
}

fn spawn_guard(app: AppHandle) {
    let run = move || {
        let st = state(&app);
        let mut last_tick = SystemTime::now();
        let mut last_save = SystemTime::now();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = SystemTime::now();
            let gap = now.duration_since(last_tick).unwrap_or_default();
            last_tick = now;

            let settings = {
                let i = st.inner();
                match &i.vault {
                    Some(v) if !i.locked => Some(v.settings.clone()),
                    _ => None,
                }
            };
            let Some(s) = settings else { continue };

            if s.lock_on_screen_lock && (gap > SLEEP_GAP || guard::screen_locked()) {
                lock_now(&app);
                continue;
            }
            let Some(win) = app.get_window(MAIN) else { continue };
            let modal = st.modal.load(SeqCst);
            let hidden = win.is_minimized().unwrap_or(false) || !win.is_visible().unwrap_or(true);
            if s.lock_on_hide && hidden && !modal {
                lock_now(&app);
                continue;
            }
            let last = (*relock(&st.last_active)).max(guard::last_input());
            let idle = now.duration_since(last).unwrap_or_default();
            if idle > Duration::from_secs(u64::from(s.auto_lock_minutes) * 60) {
                lock_now(&app);
                continue;
            }
            if now.duration_since(last_save).unwrap_or_default() > SAVE_EVERY {
                last_save = now;
                save_sessions(&app);
            }
        }
    };
    std::thread::Builder::new()
        .name("chatly-auto-lock".into())
        .spawn(run)
        .expect("couldn't start the auto-lock thread");
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, keys: &str| MenuItemBuilder::with_id(id, text).accelerator(keys).build(app);
    let app_menu = SubmenuBuilder::new(app, "Chatly")
        .about(None)
        .separator()
        .item(&item("settings", "Settings…", "CmdOrCtrl+,")?)
        .item(&item("lock", "Lock Chatly", "CmdOrCtrl+L")?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item("svc:instagram", "Instagram", "CmdOrCtrl+1")?)
        .item(&item("svc:facebook", "Facebook", "CmdOrCtrl+2")?)
        .separator()
        .item(&item("nav:messages", "Messages", "CmdOrCtrl+Shift+M")?)
        .item(&item("nav:home", "Home", "CmdOrCtrl+Shift+H")?)
        .separator()
        .item(&item("nav:back", "Back", "CmdOrCtrl+[")?)
        .item(&item("nav:forward", "Forward", "CmdOrCtrl+]")?)
        .item(&item("nav:reload", "Reload", "CmdOrCtrl+R")?)
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .fullscreen()
        .build()?;
    MenuBuilder::new(app).items(&[&app_menu, &edit, &view, &window]).build()
}

fn on_menu(app: &AppHandle, id: &str) {
    if id == "lock" {
        return lock_now(app);
    }
    let app = app.clone();
    let id = id.to_string();
    std::thread::spawn(move || {
        if id == "settings" {
            let _ = set_overlay_now(&app, true);
        } else if let Some(svc) = id.strip_prefix("svc:").and_then(Service::from_id) {
            let _ = select(&app, svc);
        } else if let Some(action) = id.strip_prefix("nav:") {
            navigate_active(&app, action);
        }
    });
}

fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN {
        return;
    }
    let app = window.app_handle();
    match event {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => layout(app),
        WindowEvent::Focused(false) => {
            let st = state(app);
            let lock_on_blur = st.inner().vault.as_ref().is_some_and(|v| v.settings.lock_on_blur);
            if lock_on_blur && !st.modal.load(SeqCst) {
                lock_now(app);
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            quit(app);
        }
        _ => {}
    }
}

fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            status,
            setup,
            unlock,
            unlock_touch_id,
            lock,
            select_service,
            set_overlay,
            update_settings,
            change_password,
            set_touch_id,
            sign_out,
            erase_everything,
            security_check,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            make_private_dir(&data_dir)?;
            let vault_path = data_dir.join("vault.json");
            let vault = match Vault::load(&vault_path) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("chatly: {e}");
                    let aside = data_dir.join(format!("vault.damaged-{}.json", unix_now()));
                    let _ = std::fs::rename(&vault_path, aside);
                    None
                }
            };
            let hide_from_capture = vault.as_ref().is_none_or(|v| v.settings.hide_from_capture);
            let active = vault.as_ref().map(|v| v.last_service).unwrap_or_default();
            app.manage(Arc::new(Chatly {
                vault_path,
                inner: Mutex::new(Inner { vault, locked: true, overlay: false, active, open: HashSet::new() }),
                key: Mutex::new(None),
                last_active: Mutex::new(SystemTime::now()),
                sessions: Mutex::new(()),
                modal: AtomicBool::new(false),
                exiting: AtomicBool::new(false),
            }));

            app.set_menu(build_menu(&handle)?)?;

            let win = WindowBuilder::new(app, MAIN)
                .title("Chatly")
                .inner_size(1180.0, 800.0)
                .min_inner_size(760.0, 540.0)
                .title_bar_style(TitleBarStyle::Overlay)
                .hidden_title(true)
                .content_protected(hide_from_capture)
                .build()?;
            let size: LogicalSize<f64> = win.inner_size()?.to_logical(win.scale_factor()?);
            let shell = WebviewBuilder::new(SHELL, WebviewUrl::App("index.html".into()))
                .incognito(true)
                .on_navigation(|url| url.scheme() == "tauri" || url.as_str() == "about:blank");
            win.add_child(shell, LogicalPosition::new(0.0, 0.0), size)?;

            #[cfg(debug_assertions)]
            if std::env::var_os("CHATLY_SELFTEST").is_some() {
                selftest::run(handle.clone());
            }
            guard::watch_input();
            spawn_guard(handle);
            Ok(())
        })
        .on_window_event(on_window_event)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .build(tauri::generate_context!())
        .expect("failed to start Chatly")
        .run(|app, event| {
            if let RunEvent::ExitRequested { api, .. } = event {
                if !state(app).exiting.load(SeqCst) {
                    api.prevent_exit();
                    quit(app);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_counts() {
        assert_eq!(unread_count("(3) Instagram"), 3);
        assert_eq!(unread_count("(12) Facebook"), 12);
        assert_eq!(unread_count("(99+) Facebook"), 99);
        assert_eq!(unread_count("Instagram"), 0);
        assert_eq!(unread_count("Messenger (2)"), 0);
    }

    #[test]
    fn durations() {
        assert_eq!(human_duration(30), "30 seconds");
        assert_eq!(human_duration(60), "1 minute");
        assert_eq!(human_duration(240), "4 minutes");
    }

    #[test]
    fn unread_counts_ignore_anything_unexpected() {
        assert_eq!(unread_count(""), 0);
        assert_eq!(unread_count("  (4) Instagram"), 4);
        assert_eq!(unread_count("( 7 ) Facebook"), 7);
        assert_eq!(unread_count("(abc) Facebook"), 0);
        assert_eq!(unread_count("(-1) Facebook"), 0);
        assert_eq!(unread_count("(5 Facebook"), 0);
        assert_eq!(unread_count("(99999999999) Facebook"), 0);
    }

    #[test]
    fn durations_round_up_to_whole_minutes() {
        assert_eq!(human_duration(0), "0 seconds");
        assert_eq!(human_duration(59), "59 seconds");
        assert_eq!(human_duration(119), "1 minute");
        assert_eq!(human_duration(120), "2 minutes");
        assert_eq!(human_duration(121), "3 minutes");
        assert_eq!(human_duration(3600), "60 minutes");
    }

    #[test]
    fn new_passwords_are_counted_in_characters() {
        assert!(check_new_password("1234567").is_err());
        assert!(check_new_password("12345678").is_ok());
        assert!(check_new_password("ПарольПа").is_ok());
        assert!(check_new_password("🔒🔒🔒🔒🔒🔒🔒").is_err());
        assert!(check_new_password(&"x".repeat(1024)).is_ok());
        assert!(check_new_password(&"x".repeat(1025)).is_err());
    }

    fn quoted(text: &str) -> Vec<String> {
        text.split('"').skip(1).step_by(2).map(str::to_string).collect()
    }

    fn capability() -> serde_json::Value {
        serde_json::from_str(include_str!("../capabilities/shell.json")).unwrap()
    }

    #[test]
    fn every_command_is_declared_in_all_three_places() {
        let mut declared = include_str!("../build.rs")
            .split_once("COMMANDS: &[&str] = &[")
            .and_then(|(_, rest)| rest.split_once("];"))
            .map(|(list, _)| quoted(list))
            .expect("COMMANDS in build.rs");
        let mut handled: Vec<String> = include_str!("main.rs")
            .split_once("tauri::generate_handler![")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(list, _)| {
                list.split(',')
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .expect("generate_handler! in main.rs");
        let mut granted: Vec<String> = capability()["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str()?.strip_prefix("allow-").map(|c| c.replace('-', "_")))
            .collect();
        declared.sort();
        handled.sort();
        granted.sort();
        assert!(!declared.is_empty());
        assert_eq!(declared, handled, "build.rs COMMANDS and generate_handler! differ");
        assert_eq!(declared, granted, "build.rs COMMANDS and capabilities/shell.json differ");
    }

    #[test]
    fn only_the_local_shell_can_call_rust() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        let capabilities: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".json"))
            .collect();
        assert_eq!(capabilities, ["shell.json"]);
        let cap = capability();
        assert_eq!(cap["webviews"], serde_json::json!([SHELL]));
        assert_eq!(cap["local"], true);
        assert!(cap.get("remote").is_none());
        assert!(cap.get("windows").is_none());
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["app"]["withGlobalTauri"], false);
        let csp = &config["app"]["security"]["csp"];
        assert_eq!(csp["script-src"], "'self'");
        assert_eq!(csp["frame-src"], "'none'");
        assert_eq!(csp["connect-src"], "ipc: http://ipc.localhost");
    }
}
