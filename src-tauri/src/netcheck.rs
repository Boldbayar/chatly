use std::{
    io,
    net::{TcpStream, ToSocketAddrs},
    process::Command,
    sync::Arc,
    time::Duration,
};

use rustls::{pki_types::ServerName, CertificateError, ClientConfig, ClientConnection, RootCertStore};
use serde::Serialize;

#[derive(Serialize)]
pub struct Finding {
    pub label: String,
    pub level: &'static str,
    pub detail: String,
}

pub fn run() -> Vec<Finding> {
    let tls: Vec<_> = ["www.instagram.com", "www.facebook.com", "web.telegram.org"]
        .into_iter()
        .map(|host| std::thread::spawn(move || tls(host)))
        .collect();
    let mut out = vec![mdm(), proxy()];
    out.extend(tls.into_iter().filter_map(|h| h.join().ok()));
    out
}

fn mdm() -> Finding {
    let label = "Device management".to_string();
    let output = Command::new("/usr/bin/profiles")
        .args(["status", "-type", "enrollment"])
        .output();
    let Ok(output) = output else {
        return Finding { label, level: "info", detail: "Couldn't check device management on this Mac.".into() };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    if text.lines().any(|l| l.trim_start().starts_with("MDM enrollment: Yes")) {
        Finding {
            label,
            level: "warn",
            detail: "This Mac is managed by an organization. Its admins can install software and \
                     certificates. Chatly keeps people at the keyboard out of your chats; it can't \
                     keep out monitoring software installed by IT."
                .into(),
        }
    } else {
        Finding { label, level: "ok", detail: "This Mac isn't enrolled in device management.".into() }
    }
}

fn proxy() -> Finding {
    let label = "Proxy".to_string();
    let Ok(output) = Command::new("/usr/sbin/scutil").arg("--proxy").output() else {
        return Finding { label, level: "info", detail: "Couldn't read the proxy settings.".into() };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let value = |key: &str| {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once(" : ")?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let on = |key: &str| value(key).as_deref() == Some("1");
    let mut via = Vec::new();
    if on("HTTPSEnable") {
        via.push(format!(
            "HTTPS proxy {}:{}",
            value("HTTPSProxy").unwrap_or_default(),
            value("HTTPSPort").unwrap_or_default()
        ));
    }
    if on("ProxyAutoConfigEnable") {
        via.push(format!("auto-config script {}", value("ProxyAutoConfigURLString").unwrap_or_default()));
    }
    if on("ProxyAutoDiscoveryEnable") {
        via.push("automatic proxy discovery".into());
    }
    if via.is_empty() {
        Finding { label, level: "ok", detail: "No system proxy is set.".into() }
    } else {
        Finding {
            label,
            level: "warn",
            detail: format!(
                "Web traffic goes through {}. Whoever runs the proxy sees which sites you use, \
                 and can read the content if it also decrypts HTTPS.",
                via.join(" and ")
            ),
        }
    }
}

enum Failure {
    Intercepted(String),
    Unreachable(String),
}

fn tls(host: &'static str) -> Finding {
    let label = format!("Connection to {}", host.trim_start_matches("www."));
    match handshake(host) {
        Ok(()) => Finding {
            label,
            level: "ok",
            detail: "Encrypted all the way to the site, with a certificate from a public authority. \
                     No HTTPS inspection on this path."
                .into(),
        },
        Err(Failure::Intercepted(why)) => Finding {
            label,
            level: "warn",
            detail: format!(
                "The certificate {why}. This network is most likely decrypting HTTPS, so its \
                 operator can read what Chatly sends and receives, messages included."
            ),
        },
        Err(Failure::Unreachable(e)) => Finding {
            label,
            level: "info",
            detail: format!("Couldn't test a direct connection ({e}). If this network forces a proxy, traffic may still be inspected."),
        },
    }
}

fn handshake(host: &'static str) -> Result<(), Failure> {
    let unreachable = |e: &dyn std::fmt::Display| Failure::Unreachable(e.to_string());

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| unreachable(&e))?
        .with_root_certificates(roots)
        .with_no_client_auth();

    let addr = (host, 443)
        .to_socket_addrs()
        .map_err(|e| unreachable(&e))?
        .next()
        .ok_or_else(|| Failure::Unreachable("no address".into()))?;
    let mut sock = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| unreachable(&e))?;
    let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(5)));

    let name = ServerName::try_from(host).map_err(|e| unreachable(&e))?;
    let mut conn = ClientConnection::new(Arc::new(config), name).map_err(|e| unreachable(&e))?;
    while conn.is_handshaking() {
        conn.complete_io(&mut sock).map_err(classify)?;
    }
    Ok(())
}

fn classify(e: io::Error) -> Failure {
    let cert_error = e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .and_then(|err| match err {
            rustls::Error::InvalidCertificate(c) => Some(c.clone()),
            _ => None,
        });
    match cert_error {
        Some(CertificateError::UnknownIssuer) => {
            Failure::Intercepted("isn't from a public certificate authority".into())
        }
        Some(CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. }) => {
            Failure::Intercepted("was issued for a different name".into())
        }
        Some(other) => Failure::Intercepted(format!("was rejected ({other:?})")),
        None => Failure::Unreachable(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn certificate(e: CertificateError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, rustls::Error::InvalidCertificate(e))
    }

    #[test]
    fn certificate_errors_mean_interception() {
        let unknown = classify(certificate(CertificateError::UnknownIssuer));
        assert!(matches!(unknown, Failure::Intercepted(why) if why.contains("public certificate authority")));
        let wrong_name = classify(certificate(CertificateError::NotValidForName));
        assert!(matches!(wrong_name, Failure::Intercepted(why) if why.contains("different name")));
        let expired = classify(certificate(CertificateError::Expired));
        assert!(matches!(expired, Failure::Intercepted(why) if why.starts_with("was rejected")));
    }

    #[test]
    fn network_errors_are_not_called_interception() {
        let refused = io::Error::new(io::ErrorKind::ConnectionRefused, "refused");
        assert!(matches!(classify(refused), Failure::Unreachable(_)));
        let other_tls = io::Error::new(io::ErrorKind::InvalidData, rustls::Error::DecryptError);
        assert!(matches!(classify(other_tls), Failure::Unreachable(_)));
    }
}
