use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    #[default]
    Instagram,
    Facebook,
}

impl Service {
    pub const ALL: [Service; 2] = [Service::Instagram, Service::Facebook];

    pub fn id(self) -> &'static str {
        match self {
            Service::Instagram => "instagram",
            Service::Facebook => "facebook",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Service::Instagram => "svc-instagram",
            Service::Facebook => "svc-facebook",
        }
    }

    pub fn home(self) -> Url {
        let url = match self {
            Service::Instagram => "https://www.instagram.com/",
            Service::Facebook => "https://www.facebook.com/",
        };
        Url::parse(url).expect("static URL")
    }

    pub fn messages(self) -> Url {
        let url = match self {
            Service::Instagram => "https://www.instagram.com/direct/inbox/",
            Service::Facebook => "https://www.facebook.com/messages/",
        };
        Url::parse(url).expect("static URL")
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

const CAPTCHA_DOMAINS: &[&str] = &["recaptcha.net", "hcaptcha.com", "arkoselabs.com", "funcaptcha.com"];

fn host_in(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

pub fn is_meta(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(|h| META_DOMAINS.iter().any(|d| host_in(h, d)))
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

pub fn allowed_in_app(url: &Url) -> bool {
    match url.scheme() {
        "about" => matches!(url.path(), "blank" | "srcdoc"),
        "blob" => Url::parse(url.path()).is_ok_and(|inner| is_meta(&inner)),
        "https" => is_meta(url) || is_captcha(url),
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
        assert!(is_meta(&u("https://www.instagram.com/direct/inbox/")));
        assert!(is_meta(&u("https://scontent-lax3-1.cdninstagram.com/x.jpg")));
        assert!(is_meta(&u("https://facebook.com/")));
        assert!(!is_meta(&u("http://www.facebook.com/")), "plain http is refused");
        assert!(!is_meta(&u("https://notfacebook.com/")));
        assert!(!is_meta(&u("https://facebook.com.evil.example/")));
        assert!(!is_meta(&u("https://evil.example/?facebook.com")));
    }

    #[test]
    fn in_app_rules() {
        assert!(allowed_in_app(&u("about:blank")));
        assert!(allowed_in_app(&u("https://www.google.com/recaptcha/api2/anchor")));
        assert!(!allowed_in_app(&u("https://www.google.com/search?q=x")));
        assert!(!allowed_in_app(&u("https://example.com/")));
        assert!(!allowed_in_app(&u("file:///etc/passwd")));
        assert!(!allowed_in_app(&u("data:text/html,hi")));
        assert!(allowed_in_app(&u("blob:https://www.facebook.com/1234")));
        assert!(!allowed_in_app(&u("blob:https://evil.example/1234")));
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

    #[test]
    fn service_ids_round_trip() {
        let mut ids = std::collections::HashSet::new();
        for svc in Service::ALL {
            assert!(ids.insert(svc.id()), "duplicate id {}", svc.id());
            assert_eq!(Service::from_id(svc.id()), Some(svc));
            assert_eq!(svc.label(), format!("svc-{}", svc.id()));
            let json = format!("\"{}\"", svc.id());
            assert_eq!(serde_json::to_string(&svc).unwrap(), json);
            assert_eq!(serde_json::from_str::<Service>(&json).unwrap(), svc);
        }
        assert_eq!(Service::from_id("twitter"), None);
        assert!(serde_json::from_str::<Service>("\"Instagram\"").is_err());
        assert_eq!(Service::default(), Service::Instagram);
    }

    #[test]
    fn site_pages_load_in_app() {
        for svc in Service::ALL {
            for url in [svc.home(), svc.messages()] {
                assert_eq!(url.scheme(), "https");
                assert!(is_meta(&url) && allowed_in_app(&url), "{url}");
            }
        }
    }

    #[test]
    fn content_script_lists_the_same_domains() {
        let list = include_str!("content.js")
            .split_once("const META = /(^|\\.)(")
            .and_then(|(_, rest)| rest.split_once(")$/i"))
            .map(|(list, _)| list)
            .expect("the META regex in content.js");
        let mut from_js: Vec<String> = list.split('|').map(|d| d.replace("\\.", ".")).collect();
        let mut from_rust: Vec<String> = META_DOMAINS.iter().map(|d| d.to_string()).collect();
        from_js.sort();
        from_rust.sort();
        assert_eq!(from_js, from_rust);
    }

    #[test]
    fn lookalike_hosts_are_refused() {
        assert!(is_meta(&u("https://WWW.FACEBOOK.COM/")));
        assert!(!is_meta(&u("https://www.facebook.com@evil.example/")));
        assert!(!is_meta(&u("https://evil.example/www.facebook.com")));
        assert!(!is_meta(&u("https://xfacebook.com/")));
        assert!(!is_meta(&u("https://facebook.com-evil.example/")));
        assert!(!is_meta(&u("wss://www.facebook.com/")));
    }

    #[test]
    fn captcha_frames_only_from_captcha_hosts() {
        assert!(allowed_in_app(&u("https://www.gstatic.com/recaptcha/releases/x/recaptcha__en.js")));
        assert!(allowed_in_app(&u("https://www.recaptcha.net/recaptcha/api2/anchor")));
        assert!(allowed_in_app(&u("https://newassets.hcaptcha.com/captcha/v1/x")));
        assert!(allowed_in_app(&u("https://client-api.arkoselabs.com/fc/gc/")));
        assert!(!allowed_in_app(&u("http://www.google.com/recaptcha/api2/anchor")));
        assert!(!allowed_in_app(&u("https://www.gstatic.com/images/x.png")));
        assert!(!allowed_in_app(&u("https://www.google.com/recaptcha-evil/")));
        assert!(!allowed_in_app(&u("https://hcaptcha.com.evil.example/")));
    }

    #[test]
    fn special_urls_are_refused_unless_listed() {
        assert!(allowed_in_app(&u("about:srcdoc")));
        assert!(!allowed_in_app(&u("about:config")));
        assert!(!allowed_in_app(&u("blob:http://www.facebook.com/1234")));
        assert!(!allowed_in_app(&u("blob:null/1234")));
        assert!(!allowed_in_app(&u("javascript:alert(1)")));
        assert!(!allowed_in_app(&u("tauri://localhost/index.html")));
        assert!(!allowed_in_app(&u("http://www.instagram.com/")));
    }

    #[test]
    fn every_shim_host_is_unwrapped() {
        for host in ["l.instagram.com", "l.facebook.com", "lm.facebook.com", "l.messenger.com"] {
            let shim = u(&format!("https://{host}/l.php?u=https%3A%2F%2Fexample.com%2F"));
            assert_eq!(unwrap_link_shim(&shim).unwrap().as_str(), "https://example.com/", "{host}");
        }
        let http = unwrap_link_shim(&u("https://l.facebook.com/l.php?u=http%3A%2F%2Fexample.com%2F")).unwrap();
        assert_eq!(http.scheme(), "http");
        assert!(unwrap_link_shim(&u("https://l.facebook.com/l.php?h=AT1")).is_none());
        assert!(unwrap_link_shim(&u("https://l.facebook.com/l.php?u=%2Frelative")).is_none());
        assert!(unwrap_link_shim(&u("https://l.facebook.com/l.php?u=file%3A%2F%2F%2Fetc%2Fpasswd")).is_none());
        assert!(unwrap_link_shim(&u("https://l.instagram.com.evil.example/?u=https://x.y")).is_none());
    }

    #[test]
    fn tracking_removal_keeps_everything_else() {
        let plain = strip_tracking(u("https://example.com/a#frag"));
        assert_eq!(plain.as_str(), "https://example.com/a#frag");
        let cleaned = strip_tracking(u("https://example.com/a?igshid=1&q=x&q=y&fbclid=2#top"));
        assert_eq!(cleaned.as_str(), "https://example.com/a?q=x&q=y#top");
        let spaces = strip_tracking(u("https://example.com/s?q=a+b&fbclid=1"));
        assert_eq!(spaces.query_pairs().find(|(k, _)| k == "q").unwrap().1, "a b");
    }
}
