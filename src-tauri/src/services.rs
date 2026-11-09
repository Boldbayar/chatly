use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    #[default]
    Instagram,
    Facebook,
    Telegram,
}

const CONTENT_SCRIPT: &str = include_str!("content.js");
const LINK_DOMAINS_SLOT: &str = "__CHATLY_LINK_DOMAINS__";

impl Service {
    pub const ALL: [Service; 3] = [Service::Instagram, Service::Facebook, Service::Telegram];

    pub fn id(self) -> &'static str {
        match self {
            Service::Instagram => "instagram",
            Service::Facebook => "facebook",
            Service::Telegram => "telegram",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Service::Instagram => "svc-instagram",
            Service::Facebook => "svc-facebook",
            Service::Telegram => "svc-telegram",
        }
    }

    pub fn home(self) -> Url {
        let url = match self {
            Service::Instagram => "https://www.instagram.com/",
            Service::Facebook => "https://www.facebook.com/",
            Service::Telegram => "https://web.telegram.org/a/",
        };
        Url::parse(url).expect("static URL")
    }

    pub fn messages(self) -> Url {
        let url = match self {
            Service::Instagram => "https://www.instagram.com/direct/inbox/",
            Service::Facebook => "https://www.facebook.com/messages/",
            Service::Telegram => "https://web.telegram.org/a/",
        };
        Url::parse(url).expect("static URL")
    }

    pub fn domains(self) -> &'static [&'static str] {
        match self {
            Service::Instagram | Service::Facebook => META_DOMAINS,
            Service::Telegram => TELEGRAM_DOMAINS,
        }
    }

    pub fn link_domains(self) -> &'static [&'static str] {
        match self {
            Service::Instagram | Service::Facebook => META_DOMAINS,
            Service::Telegram => TELEGRAM_LINK_DOMAINS,
        }
    }

    pub fn owns(self, url: &Url) -> bool {
        url.scheme() == "https" && url.host_str().is_some_and(|h| self.domains().iter().any(|d| host_in(h, d)))
    }

    pub fn storage_origin(self) -> Option<&'static str> {
        match self {
            Service::Instagram | Service::Facebook => None,
            Service::Telegram => Some("https://web.telegram.org"),
        }
    }

    pub fn unread_in_title(self) -> bool {
        match self {
            Service::Instagram | Service::Facebook => true,
            Service::Telegram => false,
        }
    }

    pub fn content_script(self) -> String {
        let domains = serde_json::to_string(self.link_domains()).expect("a list of strings serializes");
        CONTENT_SCRIPT.replace(LINK_DOMAINS_SLOT, &domains)
    }
}

const META_DOMAINS: &[&str] = &[
    "instagram.com",
    "cdninstagram.com",
    "facebook.com",
    "facebook.net",
    "fbcdn.net",
    "fbsbx.com",
    "fb.com",
    "fb.me",
    "messenger.com",
    "meta.com",
    "m.me",
];

const TELEGRAM_DOMAINS: &[&str] = &["web.telegram.org"];

const TELEGRAM_LINK_DOMAINS: &[&str] = &["web.telegram.org", "t.me", "telegram.me", "telegram.dog"];

const CAPTCHA_DOMAINS: &[&str] = &["recaptcha.net", "hcaptcha.com", "arkoselabs.com", "funcaptcha.com"];

fn host_in(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

fn is_captcha(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let (Some(host), path) = (url.host_str(), url.path()) else {
        return false;
    };
    ((host == "www.google.com" || host == "www.gstatic.com") && path.starts_with("/recaptcha/"))
        || CAPTCHA_DOMAINS.iter().any(|d| host_in(host, d))
}

pub fn allowed_in_app(svc: Service, url: &Url) -> bool {
    match url.scheme() {
        "about" => matches!(url.path(), "blank" | "srcdoc"),
        "blob" => Url::parse(url.path()).is_ok_and(|inner| svc.owns(&inner)),
        "https" => svc.owns(url) || is_captcha(url),
        _ => false,
    }
}

pub fn unwrap_link_shim(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let shim = matches!(host, "l.instagram.com" | "l.facebook.com" | "lm.facebook.com" | "l.messenger.com");
    if !shim {
        return None;
    }
    let target = url.query_pairs().find(|(k, _)| k == "u")?.1;
    let target = Url::parse(&target).ok()?;
    matches!(target.scheme(), "http" | "https").then_some(target)
}

pub fn strip_tracking(mut url: Url) -> Url {
    const TRACKERS: &[&str] = &["fbclid", "igsh", "igshid"];
    if url.query().is_none() {
        return url;
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !TRACKERS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn meta_hosts() {
        let ig = Service::Instagram;
        assert!(ig.owns(&u("https://www.instagram.com/direct/inbox/")));
        assert!(ig.owns(&u("https://scontent-lax3-1.cdninstagram.com/x.jpg")));
        assert!(ig.owns(&u("https://facebook.com/")));
        assert!(!ig.owns(&u("http://www.facebook.com/")), "plain http is refused");
        assert!(!ig.owns(&u("https://notfacebook.com/")));
        assert!(!ig.owns(&u("https://facebook.com.evil.example/")));
        assert!(!ig.owns(&u("https://evil.example/?facebook.com")));
    }

    #[test]
    fn telegram_hosts() {
        let tg = Service::Telegram;
        assert!(tg.owns(&u("https://web.telegram.org/a/")));
        assert!(tg.owns(&u("https://web.telegram.org/k/#@durov")));
        assert!(!tg.owns(&u("http://web.telegram.org/a/")), "plain http is refused");
        assert!(!tg.owns(&u("https://telegram.org/faq")), "only Telegram Web loads in place");
        assert!(!tg.owns(&u("https://t.me/durov")));
        assert!(!tg.owns(&u("https://telegra.ph/x")));
        assert!(!tg.owns(&u("https://web.telegram.org.evil.example/")));
        assert!(!tg.owns(&u("https://notweb.telegram.org/")));
        assert!(!tg.owns(&u("https://evil.example/?web.telegram.org")));
    }

    #[test]
    fn sites_stay_apart() {
        assert!(!allowed_in_app(Service::Telegram, &u("https://www.instagram.com/")));
        assert!(!allowed_in_app(Service::Telegram, &u("https://www.facebook.com/")));
        assert!(!allowed_in_app(Service::Instagram, &u("https://web.telegram.org/a/")));
        assert!(!allowed_in_app(Service::Facebook, &u("https://web.telegram.org/a/")));
        assert!(!allowed_in_app(Service::Telegram, &u("blob:https://www.facebook.com/1234")));
        assert!(allowed_in_app(Service::Telegram, &u("blob:https://web.telegram.org/1234")));
    }

    #[test]
    fn link_domains_cover_domains() {
        for svc in Service::ALL {
            for d in svc.domains() {
                assert!(svc.link_domains().contains(d), "{} link domains miss {d}", svc.id());
            }
        }
    }

    #[test]
    fn content_script_gets_each_sites_domains() {
        for svc in Service::ALL {
            let script = svc.content_script();
            assert!(!script.contains(LINK_DOMAINS_SLOT), "{} script still has the placeholder", svc.id());
            for d in svc.link_domains() {
                assert!(script.contains(&format!("\"{d}\"")), "{} script misses {d}", svc.id());
            }
        }
        assert!(!Service::Telegram.content_script().contains("instagram.com"));
        assert!(!Service::Instagram.content_script().contains("t.me"));
    }

    #[test]
    fn ids_round_trip() {
        for svc in Service::ALL {
            assert_eq!(Service::from_id(svc.id()), Some(svc));
            assert_eq!(serde_json::to_string(&svc).unwrap(), format!("\"{}\"", svc.id()));
        }
    }

    #[test]
    fn in_app_rules() {
        let ig = Service::Instagram;
        assert!(allowed_in_app(ig, &u("about:blank")));
        assert!(allowed_in_app(ig, &u("https://www.google.com/recaptcha/api2/anchor")));
        assert!(allowed_in_app(Service::Telegram, &u("https://www.google.com/recaptcha/api2/anchor")));
        assert!(!allowed_in_app(ig, &u("https://www.google.com/search?q=x")));
        assert!(!allowed_in_app(ig, &u("https://example.com/")));
        assert!(!allowed_in_app(ig, &u("file:///etc/passwd")));
        assert!(!allowed_in_app(ig, &u("data:text/html,hi")));
        assert!(allowed_in_app(ig, &u("blob:https://www.facebook.com/1234")));
        assert!(!allowed_in_app(ig, &u("blob:https://evil.example/1234")));
    }

    #[test]
    fn link_shims() {
        let shim = u("https://l.instagram.com/?u=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1&e=AT0");
        assert_eq!(unwrap_link_shim(&shim).unwrap().as_str(), "https://example.com/a?b=1");
        let fb = u("https://l.facebook.com/l.php?u=https%3A%2F%2Fexample.org%2F&h=AT1");
        assert_eq!(unwrap_link_shim(&fb).unwrap().as_str(), "https://example.org/");
        let js = u("https://l.facebook.com/l.php?u=javascript%3Aalert(1)");
        assert!(unwrap_link_shim(&js).is_none());
        assert!(unwrap_link_shim(&u("https://www.facebook.com/?u=https://x.y")).is_none());
    }

    #[test]
    fn tracking_params() {
        let cleaned = strip_tracking(u("https://example.com/p?id=7&fbclid=abc&igsh=z"));
        assert_eq!(cleaned.as_str(), "https://example.com/p?id=7");
        let bare = strip_tracking(u("https://example.com/p?fbclid=abc"));
        assert_eq!(bare.as_str(), "https://example.com/p");
    }
}
