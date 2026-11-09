use std::{os::unix::fs::PermissionsExt, sync::Mutex, thread::sleep, time::Duration};

use tauri::{AppHandle, Manager};

use super::*;

pub static TITLE: Mutex<String> = Mutex::new(String::new());

const PASSWORD: &str = "selftest-password-1";

struct Report {
    failures: u32,
}

impl Report {
    fn check(&mut self, ok: bool, what: &str) {
        eprintln!("[selftest] {} {what}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            self.failures += 1;
        }
    }
}

fn cookie_values(app: &AppHandle, svc: Service, name: &str) -> Vec<(String, String)> {
    app.get_webview(svc.label())
        .and_then(|wv| wv.cookies().ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.name() == name)
        .map(|c| (c.domain().unwrap_or("").to_string(), c.value().to_string()))
        .collect()
}

fn url_of(app: &AppHandle, svc: Service) -> String {
    app.get_webview(svc.label())
        .and_then(|wv| wv.url().ok())
        .map(|u| u.to_string())
        .unwrap_or_default()
}

pub fn run(app: AppHandle) {
    std::thread::spawn(move || {
        sleep(Duration::from_secs(2));
        let st = state(&app);
        if st.inner().vault.is_some() {
            eprintln!("[selftest] refusing to run: a vault already exists at {}", st.vault_path.display());
            return;
        }
        let mut r = Report { failures: 0 };

        let (vault, key) = Vault::create(PASSWORD).expect("create vault");
        vault.save(&st.vault_path).expect("save vault");
        st.inner().vault = Some(vault);
        r.check(finish_unlock(&app, key).is_ok(), "setup unlocks and opens Instagram");
        let mode = std::fs::metadata(&st.vault_path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
        r.check(mode == 0o600, &format!("vault file is owner-only (mode {mode:o})"));

        sleep(Duration::from_secs(12));
        let ig = Service::Instagram;
        r.check(url_of(&app, ig).starts_with("https://www.instagram.com/"), &format!("Instagram loaded ({})", url_of(&app, ig)));
        let csrf = cookie_values(&app, ig, "csrftoken");
        let mid = cookie_values(&app, ig, "mid");
        eprintln!("[selftest] instagram cookies: csrftoken={} mid={}", csrf.len(), mid.len());
        r.check(!csrf.is_empty(), "Instagram set a csrftoken cookie");

        if let Some(wv) = app.get_webview(ig.label()) {
            let _ = wv.eval("location.href = 'https://example.com/'");
        }
        sleep(Duration::from_secs(3));
        r.check(url_of(&app, ig).starts_with("https://www.instagram.com/"), "off-site navigation stays blocked");

        if let Some(wv) = app.get_webview(ig.label()) {
            let _ = wv.eval(
                r#"(async () => {
                  const out = [];
                  try {
                    const res = await fetch('ipc://localhost/status', { method: 'POST', body: '{}',
                      headers: { 'Content-Type': 'application/json', 'Tauri-Callback': '1', 'Tauri-Error': '2' } });
                    out.push('fetch=' + res.status);
                  } catch (e) { out.push('fetch=blocked'); }
                  const timeout = new Promise((_, no) => setTimeout(() => no(new Error('timeout')), 3000));
                  for (const cmd of ['status', 'lock', 'plugin:dialog|message']) {
                    try {
                      await Promise.race([window.__TAURI_INTERNALS__.invoke(cmd, { message: 'x' }), timeout]);
                      out.push(cmd + '=ALLOWED');
                    } catch (e) { out.push(cmd + '=denied:' + String(e).slice(0, 60)); }
                  }
                  document.title = 'E2E:' + out.join(' | ');
                })()"#,
            );
        }
        sleep(Duration::from_secs(8));
        let probe = TITLE.lock().map(|t| t.clone()).unwrap_or_default();
        eprintln!("[selftest] IPC probe from instagram.com: {probe}");
        r.check(!probe.is_empty(), "IPC probe ran");
        r.check(!probe.contains("ALLOWED") && !probe.contains("fetch=200"), "every IPC call from instagram.com is refused");
        r.check(!st.inner().locked, "the page couldn't lock Chatly through IPC");

        lock_now(&app);
        sleep(Duration::from_secs(3));
        r.check(st.key().is_none(), "key is dropped after locking");
        let disk = std::fs::read_to_string(&st.vault_path).unwrap_or_default();
        r.check(st.inner().vault.as_ref().is_some_and(|v| v.sessions.is_some()), "sessions saved to the vault");
        let leaked = csrf.iter().any(|(_, v)| !v.is_empty() && disk.contains(v.as_str())) || disk.contains("csrftoken");
        r.check(!leaked, "no cookie names or values in the vault file");

        let vault = st.inner().vault.clone().expect("vault");
        r.check(matches!(vault.unlock("not-the-password"), Err(vault::Error::WrongPassword)), "wrong password is refused");
        let key = vault.unlock(PASSWORD).expect("unlock");
        r.check(finish_unlock(&app, key).is_ok(), "right password unlocks");

        {
            let _sessions = relock(&st.sessions);
            if let Some(wv) = app.get_webview(ig.label()) {
                let _ = wv.close();
            }
            st.inner().open.remove(&ig);
        }
        r.check(ensure_webview(&app, ig).is_ok(), "Instagram webview recreated from the vault");
        layout(&app);
        sleep(Duration::from_secs(10));
        let csrf_after = cookie_values(&app, ig, "csrftoken");
        let mid_after = cookie_values(&app, ig, "mid");
        eprintln!("[selftest] after restore: csrftoken {csrf_after:?} vs {csrf:?}; mid {} vs {}", mid_after.len(), mid.len());
        r.check(
            csrf_after.len() == 1 && csrf_after == csrf,
            "restored cookies were sent to www.instagram.com and kept (no new csrftoken issued)",
        );
        r.check(mid_after == mid, "restored mid cookie kept");

        let fb = Service::Facebook;
        r.check(select(&app, fb).is_ok(), "switch to Facebook");
        sleep(Duration::from_secs(10));
        r.check(url_of(&app, fb).starts_with("https://www.facebook.com/"), &format!("Facebook loaded ({})", url_of(&app, fb)));
        let fb_has_ig = app
            .get_webview(fb.label())
            .and_then(|wv| wv.cookies().ok())
            .unwrap_or_default()
            .iter()
            .any(|c| c.domain().is_some_and(|d| d.contains("instagram")));
        r.check(!fb_has_ig, "Facebook's store has no Instagram cookies");

        if let Some(win) = app.get_window(MAIN) {
            let _ = win.minimize();
            sleep(Duration::from_secs(3));
            r.check(st.inner().locked, "minimizing locks Chatly");
            r.check(app.get_webview(fb.label()).is_some_and(|wv| wv.cookies().is_ok()), "chats stay loaded (hidden) while locked");
            let _ = win.unminimize();
            sleep(Duration::from_secs(1));
        }

        let key = st.inner().vault.clone().expect("vault").unlock(PASSWORD).expect("unlock");
        r.check(finish_unlock(&app, key).is_ok(), "unlock again");
        if let Some(v) = st.inner().vault.as_mut() {
            v.settings.auto_lock_minutes = 1;
        }
        sleep(Duration::from_secs(50));
        r.check(!st.inner().locked, "still unlocked after 50 idle seconds");
        sleep(Duration::from_secs(15));
        r.check(st.inner().locked, "auto-locked after 1 idle minute");

        erase_everything_now(&app);
        r.check(!st.vault_path.exists(), "erase deletes the vault");
        r.check(app.get_webview(ig.label()).is_none() && app.get_webview(fb.label()).is_none(), "erase closes the service webviews");

        eprintln!("[selftest] done: {} failure(s)", r.failures);
        st.exiting.store(true, SeqCst);
        app.exit(if r.failures == 0 { 0 } else { 1 });
    });
}
