use super::*;
use crate::RequestSettings;
use std::path::PathBuf;

fn pem(host_pattern: &str, name: &str) -> ClientCertificate {
    ClientCertificate {
        host_pattern: host_pattern.to_string(),
        kind: ClientCertificateKind::Pem {
            cert_path: PathBuf::from(format!("/certs/{name}.crt")),
            key_path: PathBuf::from(format!("/certs/{name}.key")),
        },
    }
}

fn picked<'a>(settings: &'a NetworkSettings, host: &str) -> Option<&'a str> {
    settings
        .certificate_for(host)
        .map(|certificate| certificate.host_pattern.as_str())
}

#[test]
fn defaults_are_system_proxy_and_no_certs() {
    let defaults = NetworkSettings::default();
    assert_eq!(defaults.proxy.mode, ProxyMode::System);
    assert!(defaults.proxy.no_proxy.is_empty());
    assert!(defaults.client_certificates.is_empty());
    assert_eq!(defaults.ca_bundle_path, None);

    // An empty or absent section reads as the same defaults, so a settings file that
    // predates the network section keeps working.
    let from_empty: NetworkSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(from_empty, defaults);

    // The bundled settings file documents exactly these defaults.
    let bundled: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/settings/default.json")).unwrap();
    let bundled_network: NetworkSettings =
        serde_json::from_value(bundled["network"].clone()).unwrap();
    assert_eq!(bundled_network, defaults);
    assert_eq!(
        bundled["network"],
        serde_json::to_value(&defaults).unwrap(),
        "default.json spells out every default key"
    );

    // A request inherits the global proxy decision unless it says otherwise.
    assert_eq!(RequestSettings::default().proxy_bypass, None);
    let bypassing: RequestSettings = serde_json::from_str(r#"{"proxy_bypass":true}"#).unwrap();
    assert_eq!(bypassing.proxy_bypass, Some(true));
    assert!(bypassing.extra.is_empty());
}

#[test]
fn no_proxy_matches_suffix_and_exact() {
    let proxy = ProxyConfig {
        mode: ProxyMode::System,
        no_proxy: vec![
            "localhost".to_string(),
            ".internal.test".to_string(),
            "*.corp.test".to_string(),
            "Example.COM".to_string(),
            "10.0.0.7".to_string(),
            "  ".to_string(),
        ],
    };

    let cases = [
        ("localhost", true),
        ("LOCALHOST", true),
        ("localhost.", true),
        ("api.internal.test", true),
        ("internal.test", true),
        ("a.b.corp.test", true),
        ("corp.test", true),
        ("example.com", true),
        ("www.example.com", true),
        ("10.0.0.7", true),
        // Suffixes only match on a label boundary.
        ("badexample.com", false),
        ("notlocalhost", false),
        ("xinternal.test", false),
        ("10.0.0.70", false),
        ("example.org", false),
        ("", false),
    ];
    for (host, expected) in cases {
        assert_eq!(proxy.matches_no_proxy(host), expected, "host {host:?}");
    }

    let everything = ProxyConfig {
        mode: ProxyMode::System,
        no_proxy: vec!["*".to_string()],
    };
    assert!(everything.matches_no_proxy("anything.test"));

    assert!(!ProxyConfig::default().matches_no_proxy("localhost"));
}

#[test]
fn certificate_for_host_picks_most_specific() {
    let settings = NetworkSettings {
        client_certificates: vec![
            pem("*.example.com", "wildcard"),
            pem("*.api.example.com", "deeper-wildcard"),
            pem("api.example.com", "exact"),
            pem("API.example.com", "duplicate-exact"),
            pem("other.test", "other"),
        ],
        ..NetworkSettings::default()
    };

    let cases = [
        ("api.example.com", Some("api.example.com")),
        ("Api.Example.Com", Some("api.example.com")),
        ("api.example.com.", Some("api.example.com")),
        ("v1.api.example.com", Some("*.api.example.com")),
        ("www.example.com", Some("*.example.com")),
        ("a.b.example.com", Some("*.example.com")),
        // A wildcard covers subdomains, not the bare domain.
        ("example.com", None),
        ("other.test", Some("other.test")),
        ("sub.other.test", None),
        ("badexample.com", None),
        ("", None),
    ];
    for (host, expected) in cases {
        assert_eq!(picked(&settings, host), expected, "host {host:?}");
    }

    assert!(
        NetworkSettings::default()
            .certificate_for("api.example.com")
            .is_none()
    );
}

#[test]
fn network_settings_never_serialise_a_secret_value() {
    let settings = NetworkSettings {
        proxy: ProxyConfig {
            mode: ProxyMode::Manual {
                scheme: ProxyScheme::Socks5,
                host: "proxy.test".to_string(),
                port: 1080,
                auth_secret: Some(SecretRef::new("network/proxy-auth")),
            },
            no_proxy: vec!["localhost".to_string()],
        },
        client_certificates: vec![ClientCertificate {
            host_pattern: "mtls.test".to_string(),
            kind: ClientCertificateKind::Pfx {
                path: PathBuf::from("/certs/client.pfx"),
                passphrase_secret: Some(SecretRef::new("network/mtls-passphrase")),
            },
        }],
        ca_bundle_path: Some(PathBuf::from("/certs/ca.pem")),
    };

    let json = serde_json::to_value(&settings).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "proxy": {
                "mode": "manual",
                "scheme": "socks5",
                "host": "proxy.test",
                "port": 1080,
                "auth_secret": "network/proxy-auth",
                "no_proxy": ["localhost"],
            },
            "client_certificates": [{
                "host_pattern": "mtls.test",
                "kind": "pfx",
                "path": "/certs/client.pfx",
                "passphrase_secret": "network/mtls-passphrase",
            }],
            "ca_bundle_path": "/certs/ca.pem",
        })
    );
    let round_tripped: NetworkSettings = serde_json::from_value(json).unwrap();
    assert_eq!(round_tripped, settings);

    // A secret value pasted into the settings by hand is never written back out: only
    // the keychain references survive a load and save.
    let pasted = r#"{
        "proxy": {
            "mode": "manual", "scheme": "http", "host": "proxy.test", "port": 8080,
            "auth_secret": "network/proxy-auth",
            "username": "fake-user", "password": "fake-proxy-password"
        },
        "client_certificates": [{
            "host_pattern": "mtls.test", "kind": "pfx", "path": "/certs/client.pfx",
            "passphrase": "fake-pfx-passphrase"
        }]
    }"#;
    let loaded: NetworkSettings = serde_json::from_str(pasted).unwrap();
    let saved = serde_json::to_string(&loaded).unwrap();
    for value in ["fake-user", "fake-proxy-password", "fake-pfx-passphrase"] {
        assert!(!saved.contains(value), "{value} leaked into {saved}");
    }
    assert!(saved.contains("network/proxy-auth"));
}
