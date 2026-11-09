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
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|h| META_DOMAINS.iter().any(|d| host_in(h, d)))
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
    let shim = matches!(
        host,
        "l.instagram.com" | "l.facebook.com" | "lm.facebook.com" | "l.messenger.com"
    );
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
}
