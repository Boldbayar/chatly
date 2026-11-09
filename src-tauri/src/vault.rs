use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::services::Service;

pub type Key = Zeroizing<[u8; 32]>;
pub type Sessions = BTreeMap<String, Vec<StoredCookie>>;

const FORMAT_VERSION: u32 = 1;
const AAD_WRAPPED_KEY: &[u8] = b"chatly/v1/wrapped-key";
const AAD_KEY_CHECK: &[u8] = b"chatly/v1/key-check";
const AAD_SESSIONS: &[u8] = b"chatly/v1/sessions";

const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_PASSES: u32 = 3;
const KDF_LANES: u32 = 1;

const FREE_ATTEMPTS: u32 = 5;
const MAX_LOCKOUT_SECS: u64 = 60 * 60;

#[derive(Debug)]
pub enum Error {
    WrongPassword,
    Corrupt(String),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WrongPassword => write!(f, "Wrong password."),
            Error::Corrupt(what) => write!(f, "The vault file is damaged ({what})."),
            Error::Io(e) => write!(f, "Couldn't access the vault file: {e}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Kdf {
    pub alg: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

impl Kdf {
    fn fresh() -> Self {
        Kdf {
            alg: "argon2id".into(),
            m_kib: KDF_MEMORY_KIB,
            t: KDF_PASSES,
            p: KDF_LANES,
            salt: B64.encode(random::<16>()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sealed {
    pub nonce: String,
    pub ct: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub auto_lock_minutes: u32,
    pub lock_on_screen_lock: bool,
    pub lock_on_hide: bool,
    pub lock_on_blur: bool,
    pub hide_from_capture: bool,
    pub confirm_links: bool,
    pub remember_sessions: bool,
    pub wipe_after_failures: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_lock_minutes: 5,
            lock_on_screen_lock: true,
            lock_on_hide: true,
            lock_on_blur: false,
            hide_from_capture: true,
            confirm_links: true,
            remember_sessions: true,
            wipe_after_failures: false,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.auto_lock_minutes = self.auto_lock_minutes.clamp(1, 60);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
    pub expires: Option<i64>,
}

impl StoredCookie {
    pub fn from_cookie(c: &cookie::Cookie<'_>) -> Option<Self> {
        let domain = c.domain()?.trim_start_matches('.').to_string();
        if domain.is_empty() {
            return None;
        }
        Some(StoredCookie {
            name: c.name().to_string(),
            value: c.value().to_string(),
            domain,
            path: c.path().unwrap_or("/").to_string(),
            secure: c.secure().unwrap_or(false),
            http_only: c.http_only().unwrap_or(false),
            same_site: c.same_site().map(|s| s.to_string()),
            expires: c.expires_datetime().map(|d| d.unix_timestamp()),
        })
    }

    pub fn is_expired(&self, now: i64) -> bool {
        self.expires.is_some_and(|e| e <= now)
    }

    pub fn to_cookie(&self) -> cookie::Cookie<'static> {
        let mut b = cookie::Cookie::build((self.name.clone(), self.value.clone()))
            .domain(format!("..{}", self.domain))
            .path(self.path.clone())
            .secure(self.secure)
            .http_only(self.http_only);
        match self.same_site.as_deref() {
            Some("Strict") => b = b.same_site(cookie::SameSite::Strict),
            Some("Lax") => b = b.same_site(cookie::SameSite::Lax),
            _ => {}
        }
        if let Some(at) = self
            .expires
            .and_then(|e| cookie::time::OffsetDateTime::from_unix_timestamp(e).ok())
        {
            b = b.expires(at);
        }
        b.build()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vault {
    pub version: u32,
    pub kdf: Kdf,
    pub wrapped_key: Sealed,
    pub key_check: Sealed,
    #[serde(default)]
    pub sessions: Option<Sealed>,
    #[serde(default)]
    pub touch_id: bool,
    #[serde(default)]
    pub failed_attempts: u32,
    #[serde(default)]
    pub lockout_until: u64,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub last_service: Service,
}

impl Vault {
    pub fn create(password: &str) -> Result<(Vault, Key), Error> {
        let kdf = Kdf::fresh();
        let kek = derive(password, &kdf)?;
        let key: Key = Zeroizing::new(random::<32>());
        let vault = Vault {
            version: FORMAT_VERSION,
            wrapped_key: seal(&kek, key.as_ref(), AAD_WRAPPED_KEY),
            key_check: seal(&key, b"chatly", AAD_KEY_CHECK),
            kdf,
            sessions: None,
            touch_id: false,
            failed_attempts: 0,
            lockout_until: 0,
            settings: Settings::default(),
            last_service: Service::default(),
        };
        Ok((vault, key))
    }

    pub fn unlock(&self, password: &str) -> Result<Key, Error> {
        let kek = derive(password, &self.kdf)?;
        let raw = open(&kek, &self.wrapped_key, AAD_WRAPPED_KEY).map_err(|_| Error::WrongPassword)?;
        if raw.len() != 32 {
            return Err(Error::Corrupt("key length".into()));
        }
        let mut key: Key = Zeroizing::new([0; 32]);
        key.copy_from_slice(&raw);
        Ok(key)
    }

    pub fn check_key(&self, key: &Key) -> bool {
        open(key, &self.key_check, AAD_KEY_CHECK).is_ok()
    }

    pub fn rewrap(&mut self, key: &Key, new_password: &str) -> Result<(), Error> {
        let kdf = Kdf::fresh();
        let kek = derive(new_password, &kdf)?;
        self.wrapped_key = seal(&kek, key.as_ref(), AAD_WRAPPED_KEY);
        self.kdf = kdf;
        Ok(())
    }

    pub fn read_sessions(&self, key: &Key) -> Result<Sessions, Error> {
        let Some(sealed) = &self.sessions else {
            return Ok(Sessions::new());
        };
        let raw = open(key, sealed, AAD_SESSIONS).map_err(|_| Error::Corrupt("sessions".into()))?;
        serde_json::from_slice(&raw).map_err(|e| Error::Corrupt(e.to_string()))
    }

    pub fn write_sessions(&mut self, key: &Key, sessions: &Sessions) -> Result<(), Error> {
        let json = Zeroizing::new(serde_json::to_vec(sessions).map_err(|e| Error::Corrupt(e.to_string()))?);
        self.sessions = Some(seal(key, &json, AAD_SESSIONS));
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Option<Vault>, Error> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| Error::Corrupt(e.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), Error> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| Error::Corrupt(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn lockout_remaining(&self, now: u64) -> u64 {
        self.lockout_until.saturating_sub(now)
    }

    pub fn record_failure(&mut self, now: u64) {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        if self.failed_attempts >= FREE_ATTEMPTS {
            let doublings = (self.failed_attempts - FREE_ATTEMPTS).min(7);
            let wait = (30u64 << doublings).min(MAX_LOCKOUT_SECS);
            self.lockout_until = now + wait;
        }
    }

    pub fn record_success(&mut self) {
        self.failed_attempts = 0;
        self.lockout_until = 0;
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the OS random number generator is unavailable");
    bytes
}

fn derive(password: &str, kdf: &Kdf) -> Result<Key, Error> {
    if kdf.alg != "argon2id" {
        return Err(Error::Corrupt(format!("unknown KDF {}", kdf.alg)));
    }
    let salt = B64.decode(&kdf.salt).map_err(|_| Error::Corrupt("salt".into()))?;
    let params = Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|e| Error::Corrupt(e.to_string()))?;
    let mut out: Key = Zeroizing::new([0; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &salt, out.as_mut())
        .map_err(|e| Error::Corrupt(e.to_string()))?;
    Ok(out)
}

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(key))
}

fn seal(key: &[u8; 32], msg: &[u8], aad: &[u8]) -> Sealed {
    let nonce = random::<24>();
    let ct = cipher(key)
        .encrypt(XNonce::from_slice(&nonce), Payload { msg, aad })
        .expect("XChaCha20-Poly1305 encryption does not fail");
    Sealed {
        nonce: B64.encode(nonce),
        ct: B64.encode(ct),
    }
}

fn open(key: &[u8; 32], sealed: &Sealed, aad: &[u8]) -> Result<Zeroizing<Vec<u8>>, ()> {
    let nonce = B64.decode(&sealed.nonce).map_err(|_| ())?;
    let ct = B64.decode(&sealed.ct).map_err(|_| ())?;
    if nonce.len() != 24 {
        return Err(());
    }
    cipher(key)
        .decrypt(XNonce::from_slice(&nonce), Payload { msg: &ct, aad })
        .map(Zeroizing::new)
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str) -> StoredCookie {
        StoredCookie {
            name: name.into(),
            value: "v".into(),
            domain: "instagram.com".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            same_site: Some("Lax".into()),
            expires: Some(4_000_000_000),
        }
    }

    #[test]
    fn password_round_trip_and_rejection() {
        let (vault, key) = Vault::create("correct horse battery").unwrap();
        assert_eq!(*vault.unlock("correct horse battery").unwrap(), *key);
        assert!(matches!(vault.unlock("wrong"), Err(Error::WrongPassword)));
        assert!(vault.check_key(&key));
        assert!(!vault.check_key(&Zeroizing::new([7; 32])));
    }

    #[test]
    fn sessions_are_encrypted_and_bound_to_the_key() {
        let (mut vault, key) = Vault::create("pw-for-test").unwrap();
        let mut s = Sessions::new();
        s.insert("instagram".into(), vec![cookie("sessionid")]);
        vault.write_sessions(&key, &s).unwrap();
        let on_disk = serde_json::to_string(&vault).unwrap();
        assert!(!on_disk.contains("sessionid"), "cookie names must not be stored in plain text");
        assert_eq!(vault.read_sessions(&key).unwrap()["instagram"][0].name, "sessionid");
        assert!(vault.read_sessions(&Zeroizing::new([1; 32])).is_err());
    }

    #[test]
    fn tampering_is_detected() {
        let (mut vault, key) = Vault::create("pw-for-test").unwrap();
        vault.write_sessions(&key, &Sessions::new()).unwrap();
        let sealed = vault.sessions.as_mut().unwrap();
        let mut ct = B64.decode(&sealed.ct).unwrap();
        ct[0] ^= 1;
        sealed.ct = B64.encode(ct);
        assert!(vault.read_sessions(&key).is_err());
    }

    #[test]
    fn rewrap_changes_password_but_not_data() {
        let (mut vault, key) = Vault::create("old-password").unwrap();
        let mut s = Sessions::new();
        s.insert("facebook".into(), vec![cookie("xs")]);
        vault.write_sessions(&key, &s).unwrap();
        vault.rewrap(&key, "new-password").unwrap();
        assert!(matches!(vault.unlock("old-password"), Err(Error::WrongPassword)));
        let key2 = vault.unlock("new-password").unwrap();
        assert_eq!(vault.read_sessions(&key2).unwrap()["facebook"][0].name, "xs");
    }

    #[test]
    fn lockout_backs_off() {
        let (mut vault, _) = Vault::create("pw-for-test").unwrap();
        for _ in 0..4 {
            vault.record_failure(1000);
        }
        assert_eq!(vault.lockout_remaining(1000), 0);
        vault.record_failure(1000);
        assert_eq!(vault.lockout_remaining(1000), 30);
        vault.record_failure(1000);
        assert_eq!(vault.lockout_remaining(1000), 60);
        for _ in 0..20 {
            vault.record_failure(1000);
        }
        assert_eq!(vault.lockout_remaining(1000), MAX_LOCKOUT_SECS);
        vault.record_success();
        assert_eq!(vault.lockout_remaining(1000), 0);
    }

    #[test]
    fn cookie_domain_keeps_its_dot() {
        let c = cookie("sessionid").to_cookie();
        assert_eq!(c.domain(), Some(".instagram.com"));
        let back = StoredCookie::from_cookie(&c).unwrap();
        assert_eq!(back.domain, "instagram.com");
        assert_eq!(back.expires, Some(4_000_000_000));
        assert_eq!(back.same_site.as_deref(), Some("Lax"));
    }
}
