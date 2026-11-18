//! Tests for Postman v2.1/v2.0 import and v2.1 export (PRD §6.4, REQ-IO-01/02/03/06).

use std::path::PathBuf;

use crate::{
    Auth, Body, Child, Method, Request, VariableKind,
    postman::{
        ImportError, ImportLimits, export_collection, import_collection, import_environment,
        import_globals,
    },
};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/postman")
        .join(name);
    std::fs::read(path).expect("fixture")
}

#[test]
fn imports_v21_collection_with_folders_and_auth() {
    let (collection, report) =
        import_collection(&fixture("collection-v2.1.json"), &ImportLimits::default())
            .expect("a small v2.1 collection imports");
    assert_eq!(collection.name, "Users API");
    assert_eq!(
        collection.description.as_deref(),
        Some("A small v2.1 collection")
    );
    assert!(
        matches!(
            collection.auth,
            Auth::Basic {
                ref username,
                ref password,
                ..
            } if username.as_deref() == Some("alice")
                && password.as_deref() == Some("password-REDACTED")
        ),
        "collection-level basic auth is imported: {:?}",
        collection.auth
    );
    assert_eq!(collection.children.len(), 2, "one folder and one request");
    let Child::Folder(folder) = &collection.children[0] else {
        panic!("the first child is the Auth folder");
    };
    assert_eq!(folder.name, "Auth");
    assert_eq!(folder.order.len(), folder.children.len(), "order matches");
    let Child::Request(Request::Http(login)) = &folder.children[0] else {
        panic!("the folder holds the Login request");
    };
    assert_eq!(login.name, "Login");
    assert_eq!(login.method, Method::Post);
    assert_eq!(login.url, "{{baseUrl}}/login");
    assert!(matches!(login.auth, Auth::Bearer { .. }), "request auth");
    assert!(
        matches!(&login.body, Body::Raw { text, .. } if text == "{\"user\":\"alice\"}"),
        "the raw body is imported: {:?}",
        login.body
    );
    let Child::Request(Request::Http(get_user)) = &collection.children[1] else {
        panic!("the second child is Get user");
    };
    assert_eq!(get_user.url, "{{baseUrl}}/users/1?verbose=true");
    assert_eq!(get_user.examples.len(), 1, "one saved response");
    assert_eq!(get_user.examples[0].response.status, 200);
    // The secret variable keeps its kind and never its value; the shared one keeps both.
    let api_key = collection
        .variables
        .iter()
        .find(|variable| variable.key == "apiKey")
        .expect("apiKey is present");
    assert_eq!(api_key.kind, VariableKind::Secret);
    assert_eq!(api_key.value, None, "a secret never keeps its value");
    let base_url = collection
        .variables
        .iter()
        .find(|variable| variable.key == "baseUrl")
        .expect("baseUrl is present");
    assert_eq!(base_url.value.as_deref(), Some("https://api.example.com"));
    assert!(
        report
            .problems
            .iter()
            .all(|problem| problem.detail.contains("preserved verbatim, not executed")),
        "a clean file reports only the preserved scripts: {:?}",
        report.problems
    );
}

#[test]
fn imports_v20_collection() {
    let (collection, report) =
        import_collection(&fixture("collection-v2.0.json"), &ImportLimits::default())
            .expect("a v2.0 collection imports");
    assert_eq!(collection.name, "Legacy collection");
    assert_eq!(
        collection
            .extra
            .get("format_version")
            .and_then(|value| value.as_str()),
        Some("2.0"),
        "the detected version is recorded"
    );
    assert_eq!(collection.children.len(), 1);
    let Child::Request(Request::Http(ping)) = &collection.children[0] else {
        panic!("the only child is the Ping request");
    };
    assert_eq!(ping.name, "Ping");
    assert_eq!(ping.method, Method::Get);
    assert_eq!(ping.url, "https://api.example.com/ping");
    assert!(report.problems.is_empty());
}

#[test]
fn scripts_preserved_and_reported_not_executed() {
    let (collection, report) =
        import_collection(&fixture("collection-v2.1.json"), &ImportLimits::default())
            .expect("imports");
    let preserved: Vec<&str> = report
        .problems
        .iter()
        .map(|problem| problem.detail.as_str())
        .filter(|detail| detail.contains("preserved verbatim, not executed"))
        .collect();
    assert_eq!(
        preserved.len(),
        2,
        "one collection script and one request script"
    );
    assert!(
        collection.extra.contains_key("event"),
        "the collection-level script is preserved verbatim"
    );
    let Child::Folder(folder) = &collection.children[0] else {
        panic!("the Auth folder");
    };
    let Child::Request(Request::Http(login)) = &folder.children[0] else {
        panic!("the Login request");
    };
    assert!(
        login.extra.contains_key("event"),
        "the request script is preserved verbatim"
    );
    // Nothing was executed: the report says so, and the script bytes are still there.
    let events = collection
        .extra
        .get("event")
        .and_then(|value| value.as_array())
        .expect("event array");
    assert!(
        events[0].to_string().contains("never runs"),
        "the script text is carried, not run"
    );
}

#[test]
fn environment_secret_values_returned_separately() {
    let (environment, secrets, report) = import_environment(
        &fixture("environment-with-secret.json"),
        &ImportLimits::default(),
    )
    .expect("imports");
    assert_eq!(environment.name, "Staging");
    assert_eq!(
        secrets,
        vec![("apiToken".to_owned(), "token-REDACTED-example".to_owned())]
    );
    assert!(
        !environment
            .variables
            .iter()
            .any(|variable| variable.key == "apiToken"),
        "the secret is NOT part of the environment"
    );
    assert!(
        environment
            .variables
            .iter()
            .any(|variable| variable.key == "baseUrl"
                && variable.value.as_deref() == Some("https://staging.example.com")),
        "shared values stay in the environment"
    );
    assert!(
        environment
            .variables
            .iter()
            .any(|variable| variable.key == "disabledOne" && !variable.enabled),
        "the disabled flag round-trips"
    );
    assert!(report.problems.is_empty());
    // Globals take the same treatment.
    let globals = serde_json::json!({
        "values": [
            { "key": "g", "value": "v", "type": "default" },
            { "key": "gs", "value": "token-REDACTED-example", "type": "secret" }
        ]
    })
    .to_string();
    let (variables, global_secrets, _) =
        import_globals(globals.as_bytes(), &ImportLimits::default()).expect("globals import");
    assert_eq!(variables.len(), 1);
    assert_eq!(global_secrets.len(), 1);
}

#[test]
fn export_v21_roundtrips_core_fields() {
    let (collection, _) =
        import_collection(&fixture("collection-v2.1.json"), &ImportLimits::default())
            .expect("imports");
    let (bytes, report) = export_collection(&collection);
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(
        text.contains("v2.1.0/collection.json"),
        "the export names the v2.1 schema"
    );
    assert!(
        report.warnings.is_empty(),
        "an all-HTTP collection has no warning"
    );
    let (again, _) =
        import_collection(text.as_bytes(), &ImportLimits::default()).expect("re-imports");
    assert_eq!(again.name, collection.name);
    assert_eq!(again.description, collection.description);
    assert_eq!(again.children.len(), collection.children.len());
    let (Child::Request(Request::Http(first)), Child::Request(Request::Http(second))) =
        (&collection.children[1], &again.children[1])
    else {
        panic!("both sides hold the Get user request");
    };
    assert_eq!(first.name, second.name);
    assert_eq!(first.url, second.url);
    assert_eq!(first.method, second.method);
    assert_eq!(first.headers, second.headers);
    // A secret still never serializes its value on the way out.
    let api_key = again
        .variables
        .iter()
        .find(|variable| variable.key == "apiKey")
        .expect("apiKey is present");
    assert_eq!(api_key.kind, VariableKind::Secret);
    assert_eq!(api_key.value, None);
    // A collection-level script is preserved verbatim on import and must survive the export
    // too — dropping it would contradict "an export never drops it".
    assert!(
        text.contains("console.log('never runs')"),
        "the COLLECTION-level script round-trips (the request script shares the substring)"
    );
    assert_eq!(
        again.auth, collection.auth,
        "auth round-trips as the same shape"
    );
    assert!(
        again.extra.contains_key("event"),
        "the re-imported collection still carries its script"
    );
    // The export is schema-valid v2.1: an auth group is an ARRAY of {key, value} entries,
    // not a bare object.
    assert!(
        text.contains("\"basic\": [") || text.contains("\"basic\":["),
        "auth groups are exported as arrays: {text}"
    );
}

#[test]
fn export_skips_unsupported_request_types_with_warning() {
    let (mut collection, _) =
        import_collection(&fixture("collection-v2.1.json"), &ImportLimits::default())
            .expect("imports");
    collection
        .children
        .push(Child::Request(Request::Unsupported(
            serde_json::json!({ "name": "A WebSocket", "protocolProfileBehavior": {} }),
        )));
    let (bytes, report) = export_collection(&collection);
    let text = String::from_utf8(bytes).expect("utf-8");
    assert_eq!(
        report.warnings.len(),
        1,
        "the unsupported request is skipped and warned about: {:?}",
        report.warnings
    );
    assert!(
        report.warnings[0].contains("skipped"),
        "the warning names what was skipped"
    );
    assert!(
        !text.contains("A WebSocket"),
        "the skipped request is not in the export"
    );
    assert!(
        text.contains("Get user"),
        "the supported requests still export"
    );
}

#[test]
fn rejects_file_over_size_limit() {
    let limits = ImportLimits {
        max_bytes: 64,
        max_depth: 64,
    };
    let error = import_collection(&fixture("collection-v2.1.json"), &limits)
        .expect_err("an oversized file is refused before parsing");
    assert!(
        matches!(error, ImportError::TooLarge { limit: 64, .. }),
        "the size limit is reported: {error:?}"
    );
    assert!(
        matches!(error, ImportError::TooLarge { size, .. } if size > 64),
        "the observed size is reported"
    );
}

#[test]
fn rejects_nesting_over_depth_limit() {
    let error = import_collection(&fixture("deep-65.json"), &ImportLimits::default())
        .expect_err("a 65-deep file is refused before parsing");
    assert!(
        matches!(error, ImportError::TooDeep { limit: 64 }),
        "the depth limit is reported: {error:?}"
    );
    // The check happens on the raw bytes: a limit that admits the depth parses fine.
    let shallow = ImportLimits {
        max_bytes: 1024 * 1024,
        max_depth: 4096,
    };
    assert!(
        import_collection(&fixture("deep-65.json"), &shallow).is_ok(),
        "raising the limit admits the same file, so the LIMIT is what rejects it"
    );
}
