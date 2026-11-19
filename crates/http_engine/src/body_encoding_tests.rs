//! The body encoding edges the four named tests do not cover: per-field content types,
//! binary files, disabled fields, the `Host` listing, a redirect that drops the body,
//! a reference escaping the workspace root, and a user `Content-Type` on a multipart
//! body (REQ-HTTP-03/04/05).

use std::collections::BTreeMap;

use api_model::{Body, FormField, RawLanguage};
use url::Url;

use crate::body::resolve_body_files;
use crate::body_test_support::{
    entry, file_field, header_named, prepare_request, request, text_field,
};
use crate::{ReqwestTransport, TestServer, Transport, TransportError};

/// A text form-data field carries the content type its row set, in its own part.
#[tokio::test]
async fn form_data_text_field_carries_its_own_content_type()
-> Result<(), Box<dyn std::error::Error>> {
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::FormData {
            fields: vec![FormField {
                content_type: Some("application/json".to_string()),
                ..text_field("payload", "{\"ok\":true}")
            }],
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );

    let prepared = prepare_request(&request)?;

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(prepared.resolved).await?;

    let received = server.requests();
    let content_type = header_named(&received[0].headers, "Content-Type")
        .map(|header| header.value.clone())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("multipart/form-data; boundary="),
        "no boundary content type: {content_type}"
    );
    let body = String::from_utf8_lossy(&received[0].body);
    for expected in [r#"name="payload""#, "application/json", r#"{"ok":true}"#] {
        assert!(body.contains(expected), "`{expected}` missing: {body}");
    }
    Ok(())
}

/// A binary body's file reference resolves against the workspace root and its bytes
/// reach the server under the mode's content type (REQ-HTTP-05).
#[tokio::test]
async fn binary_body_file_resolves_relative_to_the_workspace_root()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("data.bin"), b"binary-payload")?;
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::Binary {
            file: "data.bin".to_string(),
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );

    let prepared = prepare_request(&request)?;

    assert_eq!(
        header_named(&prepared.resolved.auto_headers, "Content-Type")
            .map(|header| header.value.as_str()),
        Some("application/octet-stream"),
    );
    let mut resolved = prepared.resolved;
    resolved.body = resolve_body_files(resolved.body, workspace.path())?;

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(resolved).await?;

    let received = server.requests();
    assert_eq!(received[0].body, b"binary-payload".to_vec());
    Ok(())
}

/// A disabled form-data row is not sent at all — not as an empty part, not as anything.
#[tokio::test]
async fn a_disabled_form_field_is_skipped() -> Result<(), Box<dyn std::error::Error>> {
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::FormData {
            fields: vec![
                text_field("kept", "yes"),
                FormField {
                    enabled: false,
                    ..text_field("skipped", "no")
                },
            ],
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );

    let prepared = prepare_request(&request)?;

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(prepared.resolved).await?;

    let received = server.requests();
    let body = String::from_utf8_lossy(&received[0].body);
    assert!(body.contains(r#"name="kept""#), "`kept` missing: {body}");
    assert!(
        !body.contains("skipped"),
        "a disabled field reached the wire: {body}"
    );
    Ok(())
}

/// The `Host` listing names the port only when the URL's is not the scheme's default.
#[test]
fn host_header_lists_the_port_only_when_it_is_not_the_default() {
    let at_port = request(
        &Url::parse("http://example.test:8080/collect").unwrap(),
        Body::None,
        Vec::new(),
    );
    let at_default_port = request(
        &Url::parse("http://example.test:80/collect").unwrap(),
        Body::None,
        Vec::new(),
    );
    let without_port = request(
        &Url::parse("https://example.test/collect").unwrap(),
        Body::None,
        Vec::new(),
    );

    for (request, expected) in [
        (at_port, "example.test:8080"),
        (at_default_port, "example.test"),
        (without_port, "example.test"),
    ] {
        let prepared = prepare_request(&request).unwrap();
        assert_eq!(
            header_named(&prepared.resolved.auto_headers, "Host")
                .map(|header| header.value.as_str()),
            Some(expected),
        );
    }
}

/// A 303 turns the send into a body-less GET, and the auto `Content-Type` goes with the
/// body it described (REQ-HTTP-09).
#[tokio::test]
async fn a_303_redirect_drops_the_auto_content_type() -> Result<(), Box<dyn std::error::Error>> {
    let server = TestServer::start_redirects(&[303]).await?;
    let request = request(
        server.url(),
        Body::Raw {
            language: RawLanguage::Json,
            text: r#"{"name":"pustman"}"#.to_string(),
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );

    let prepared = prepare_request(&request)?;

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    let response = transport.send(prepared.resolved).await?;

    assert_eq!(response.status, 200);
    let received = server.requests();
    assert_eq!(received.len(), 2);
    assert!(
        header_named(&received[0].headers, "Content-Type").is_some(),
        "the first hop sent no content type: {:?}",
        received[0].headers
    );
    assert_eq!(received[1].method, "GET");
    assert!(received[1].body.is_empty());
    assert!(
        header_named(&received[1].headers, "Content-Type").is_none(),
        "the body-less hop kept a content type: {:?}",
        received[1].headers
    );
    Ok(())
}

/// A relative file reference that resolves outside the workspace root is refused: an
/// imported collection is untrusted data and must not read arbitrary files (REQ-HTTP-05).
#[test]
fn a_file_reference_that_escapes_the_workspace_root_is_refused() {
    let outer = tempfile::tempdir().unwrap();
    let workspace = outer.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(outer.path().join("outside.txt"), b"outside").unwrap();
    let request = request(
        &Url::parse("https://example.test/upload").unwrap(),
        Body::FormData {
            fields: vec![file_field("upload", "../outside.txt")],
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );
    let prepared = prepare_request(&request).unwrap();

    let error = resolve_body_files(prepared.resolved.body, &workspace).unwrap_err();

    match error {
        TransportError::Io { detail } => assert!(
            detail.contains("outside the workspace root"),
            "the refusal does not say why: {detail}"
        ),
        other => panic!("expected an Io refusal, got {other:?}"),
    }
}

/// A user `Content-Type` on a multipart body is the only one sent: `Content-Type` is a
/// singleton field, the user's header wins, and the user owns the consequences.
#[tokio::test]
async fn a_user_content_type_on_a_multipart_body_is_the_only_one_sent()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("payload.txt"), b"file-payload-bytes")?;
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::FormData {
            fields: vec![
                text_field("note", "hello"),
                file_field("upload", "payload.txt"),
            ],
            extra: BTreeMap::new(),
        },
        vec![entry("content-type", "text/plain")],
    );

    let prepared = prepare_request(&request)?;
    let mut resolved = prepared.resolved;
    resolved.body = resolve_body_files(resolved.body, workspace.path())?;

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(resolved).await?;

    let received = server.requests();
    let content_types: Vec<_> = received[0]
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("content-type"))
        .collect();
    assert_eq!(content_types.len(), 1, "headers: {:?}", received[0].headers);
    assert_eq!(content_types[0].value, "text/plain");
    Ok(())
}
