//! Every M1 body mode sends correctly, the headers its mode adds are listed in the
//! resolved request, and a user header of the same name wins (REQ-HTTP-03/04/05).

use std::collections::BTreeMap;

use api_model::{Body, FormField, RawLanguage};

use super::resolve_body_files;
use crate::body_test_support::{
    entry, file_field, header_named, prepare_request, request, text_field,
};
use crate::{ReqwestTransport, TestServer, Transport};

/// The JSON text the raw-body tests send.
const JSON: &str = r#"{"name":"pustman"}"#;

/// A raw JSON body lists the content type its language names — plus User-Agent, Accept
/// and Host — and the server receives that content type and the exact body.
#[tokio::test]
async fn json_raw_body_sets_content_type() -> Result<(), Box<dyn std::error::Error>> {
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::Raw {
            language: RawLanguage::Json,
            text: JSON.to_string(),
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );

    let prepared = prepare_request(&request)?;

    let listed = &prepared.resolved.auto_headers;
    assert_eq!(
        header_named(listed, "Content-Type").map(|header| header.value.as_str()),
        Some("application/json"),
    );
    assert_eq!(
        header_named(listed, "Content-Length").map(|header| header.value.clone()),
        Some(JSON.len().to_string()),
    );
    assert!(
        header_named(listed, "User-Agent")
            .is_some_and(|header| header.value.starts_with("Pustman/")),
        "no Pustman User-Agent listed: {listed:?}"
    );
    assert_eq!(
        header_named(listed, "Accept").map(|header| header.value.as_str()),
        Some("*/*"),
    );
    assert_eq!(
        header_named(listed, "Accept-Encoding").map(|header| header.value.as_str()),
        Some("gzip,deflate,br"),
    );
    let host = server.url().host_str().unwrap_or_default();
    assert_eq!(
        header_named(listed, "Host").map(|header| header.value.as_str()),
        Some(format!("{host}:{}", server.url().port().unwrap_or_default()).as_str()),
    );

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(prepared.resolved).await?;

    let received = server.requests();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].body, JSON.as_bytes());
    let content_types: Vec<_> = received[0]
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("content-type"))
        .collect();
    assert_eq!(content_types.len(), 1, "headers: {:?}", received[0].headers);
    assert_eq!(content_types[0].value, "application/json");
    Ok(())
}

/// A user `content-type` — spelled lowercase, so only case-insensitive matching can see
/// it — suppresses the auto one in the listing and is the only one the server receives.
#[tokio::test]
async fn user_content_type_overrides_auto() -> Result<(), Box<dyn std::error::Error>> {
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::Raw {
            language: RawLanguage::Json,
            text: JSON.to_string(),
            extra: BTreeMap::new(),
        },
        vec![entry("content-type", "text/plain")],
    );

    let prepared = prepare_request(&request)?;

    assert!(
        header_named(&prepared.resolved.auto_headers, "Content-Type").is_none(),
        "the user's header must win: {:?}",
        prepared.resolved.auto_headers
    );

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(prepared.resolved).await?;

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

/// A urlencoded value carrying the pair separators `&` and `=` arrives percent-encoded,
/// so one field never splits into two, and the mode's content type goes with it.
#[tokio::test]
async fn urlencoded_body_encodes_pairs() -> Result<(), Box<dyn std::error::Error>> {
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::UrlEncoded {
            fields: vec![entry("a", "x&y=z w"), entry("b", "2")],
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );

    let prepared = prepare_request(&request)?;

    assert_eq!(
        header_named(&prepared.resolved.auto_headers, "Content-Type")
            .map(|header| header.value.as_str()),
        Some("application/x-www-form-urlencoded"),
    );

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(prepared.resolved).await?;

    let received = server.requests();
    assert_eq!(received[0].body, b"a=x%26y%3Dz+w&b=2".to_vec());
    assert_eq!(
        header_named(&received[0].headers, "Content-Type").map(|header| header.value.as_str()),
        Some("application/x-www-form-urlencoded"),
    );
    Ok(())
}

/// A form-data body with one text field and one file field — the file referenced
/// relative to the workspace root, with its own content type — reaches the server as a
/// real multipart message: both parts, the file's bytes, its name and its content type.
#[tokio::test]
async fn form_data_file_field_sent() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("payload.txt"), b"file-payload-bytes")?;
    let server = TestServer::start().await?;
    let request = request(
        server.url(),
        Body::FormData {
            fields: vec![
                text_field("note", "hello"),
                FormField {
                    content_type: Some("text/markdown".to_string()),
                    ..file_field("upload", "payload.txt")
                },
            ],
            extra: BTreeMap::new(),
        },
        Vec::new(),
    );

    let prepared = prepare_request(&request)?;

    assert_eq!(
        header_named(&prepared.resolved.auto_headers, "Content-Type")
            .map(|header| header.value.as_str()),
        Some("multipart/form-data"),
    );
    let mut resolved = prepared.resolved;
    resolved.body = resolve_body_files(resolved.body, workspace.path())?;

    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    transport.send(resolved).await?;

    let received = server.requests();
    let content_type = header_named(&received[0].headers, "Content-Type")
        .map(|header| header.value.clone())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("multipart/form-data; boundary="),
        "no boundary content type: {content_type}"
    );
    let body = String::from_utf8_lossy(&received[0].body);
    for expected in [
        r#"name="note""#,
        r#"name="upload""#,
        r#"filename="payload.txt""#,
        "file-payload-bytes",
        "text/markdown",
    ] {
        assert!(body.contains(expected), "`{expected}` missing: {body}");
    }
    Ok(())
}
