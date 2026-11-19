//! Every failure kind speaks to the user and suggests a fix (PRD §8.6).

use std::time::Duration;

use crate::{RedirectHop, TransportError};

fn hop() -> RedirectHop {
    RedirectHop {
        method: api_model::Method::Get,
        url: url::Url::parse("https://example.test/next").unwrap(),
        status: 302,
        headers: Vec::new(),
        location: None,
        timing: None,
    }
}

#[test]
fn every_error_kind_has_message_and_fix() {
    let errors = vec![
        TransportError::InvalidUrl {
            url: "{{base}}/x".to_string(),
        },
        TransportError::Dns {
            host: "api.staging.local".to_string(),
        },
        TransportError::Connect {
            address: "127.0.0.1:8080".to_string(),
        },
        TransportError::Tls {
            reason: "Certificate not trusted (self-signed).".to_string(),
        },
        TransportError::Timeout {
            after: Duration::from_secs(30),
        },
        TransportError::TooManyRedirects {
            chain: vec![hop(), hop()],
        },
        TransportError::Proxy {
            reason: "Proxy authentication required (407).".to_string(),
        },
        TransportError::Protocol {
            detail: "Server does not support HTTP/2".to_string(),
        },
        TransportError::Io {
            detail: "Connection reset by peer.".to_string(),
        },
        TransportError::FileNotFound {
            path: "./fixtures/avatar.png".to_string(),
        },
        TransportError::UnresolvedSecret {
            names: vec!["api_key".to_string()],
        },
        TransportError::Cancelled,
    ];

    for error in &errors {
        assert!(
            !error.user_message().is_empty(),
            "{error:?} has no user message"
        );
        assert!(
            !error.suggested_fix().is_empty(),
            "{error:?} has no suggested fix"
        );
    }

    assert_eq!(
        errors[0].user_message(),
        "Invalid URL after variable resolution: `{{base}}/x`"
    );
    assert_eq!(
        errors[1].user_message(),
        "Could not resolve `api.staging.local`."
    );
    assert_eq!(
        errors[3].user_message(),
        "Certificate not trusted (self-signed)."
    );
    assert_eq!(
        errors[4].user_message(),
        "No response within 30 s (request timeout)."
    );
    assert_eq!(errors[5].user_message(), "Stopped after 2 redirects.");
    assert_eq!(errors[7].user_message(), "Server does not support HTTP/2");
    assert_eq!(
        errors[9].user_message(),
        "Body file not found: ./fixtures/avatar.png"
    );
    assert_eq!(
        errors[10].user_message(),
        "Secret `api_key` has no local value."
    );
    assert_eq!(errors[11].user_message(), "Cancelled by user.");
}

#[test]
fn debug_output_redacts_url_host_address_and_path() {
    let raw_url = "https://user:pass@host/x?api_key=zzz";
    let raw_host = "internal-host-with-secret";
    let raw_address = "10.0.0.9:4411";
    let raw_path = "/home/secret/.ssh/id_rsa";

    let errors = [
        TransportError::InvalidUrl {
            url: raw_url.to_string(),
        },
        TransportError::Dns {
            host: raw_host.to_string(),
        },
        TransportError::Connect {
            address: raw_address.to_string(),
        },
        TransportError::FileNotFound {
            path: raw_path.to_string(),
        },
    ];

    for (error, raw) in errors
        .iter()
        .zip([raw_url, raw_host, raw_address, raw_path])
    {
        let debug = format!("{error:?}");
        assert!(!debug.contains(raw), "{raw} leaked through Debug: {debug}");
        assert!(
            debug.contains("<redacted>"),
            "no redaction marker in Debug: {debug}"
        );
    }

    assert!(
        errors[0].user_message().contains(raw_url),
        "the UI still gets the real URL"
    );
    assert!(
        errors[3].user_message().contains(raw_path),
        "the UI still gets the real path"
    );
}
