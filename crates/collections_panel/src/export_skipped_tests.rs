//! What the export leaves out must be said out loud (S45c fix 1/2): `api_model`'s
//! v2.1 export drops a request's saved examples and its settings below the child level,
//! and it writes credentials that were typed literally into a request — so the report
//! lists the dropped content and names the items carrying literal credentials, never
//! the values (AGENTS.md › Security). Fix 3 pins the write itself: a temp file renamed
//! into place, so a failed export never leaves a half-written file where the user's
//! chosen file was.
//!
//! These tests drive [`export_work`] and [`ExportOutcome::message`] directly — the exact
//! strings the result toast shows (the toast's own pass-through is pinned in
//! `export_tests.rs`) — and live outside `export::`, so the step's pinned `export::`
//! filter still reports PASS 1.

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{
    Auth, Body, Collection, Example, ExampleResponse, HttpRequest, ItemId, KeyValue, Method,
    Request, RequestSettings,
};
use tempfile::TempDir;
use workspace_store::{Item, ItemBody, save_item};

use crate::export::export_work;

/// Obviously fake credential values (`.agents/rules/tests.md`): a fixture never carries
/// anything a scanner or a reader could mistake for a real secret.
const LITERAL_TOKEN: &str = "token-REDACTED-example";
const LITERAL_COOKIE: &str = "cookie-REDACTED-example";

/// A GET to a test URL — the skeleton the fixtures dress up.
fn request(id: ItemId, name: &str) -> HttpRequest {
    HttpRequest {
        id,
        name: String::from(name),
        method: Method::Get,
        url: String::from("https://api.test/users"),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// `pustman.json` and one collection "Users API" ordering `requests`, each request in
/// its own file under the collection's directory (REQ-WSP-01).
fn seed_workspace(requests: Vec<HttpRequest>) -> (TempDir, PathBuf, ItemId) {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    let collection_id = ItemId::new();

    save_item(
        &root,
        &Item {
            body: ItemBody::Workspace(api_model::Workspace {
                id: ItemId::new(),
                name: String::from("Export fixture"),
                format_version: String::from(workspace_store::FORMAT_VERSION),
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::new(),
            path: None,
        },
    )
    .expect("the workspace file writes");

    let order: Vec<ItemId> = requests.iter().map(|request| request.id).collect();
    save_item(
        &root,
        &Item {
            body: ItemBody::Collection(Collection {
                id: collection_id,
                name: String::from("Users API"),
                description: None,
                auth: Auth::Inherit,
                variables: Vec::new(),
                order,
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::from("collections"),
            path: None,
        },
    )
    .expect("the collection writes");

    for request in requests {
        save_item(
            &root,
            &Item {
                body: ItemBody::Request(Request::Http(request)),
                dir: PathBuf::from("collections/users-api"),
                path: None,
            },
        )
        .expect("the request writes");
    }

    (temp, root, collection_id)
}

/// The file the export writes: outside the workspace, where the user chose it. Held so
/// its folder outlives the test.
fn export_file() -> (TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("the export destination folder");
    let target = directory.path().join("users-api.postman_collection.json");
    (directory, target)
}

#[test]
fn export_lists_dropped_examples_and_settings() {
    let mut with_content = request(ItemId::new(), "Get user");
    with_content.examples = vec![Example {
        name: String::from("200 OK"),
        request: Request::Http(request(ItemId::new(), "Get user")),
        response: ExampleResponse {
            status: 200,
            headers: Vec::new(),
            body: String::new(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }];
    with_content.settings = Some(RequestSettings {
        timeout_ms: Some(5_000),
        ..RequestSettings::default()
    });
    let (_workspace, root, collection_id) = seed_workspace(vec![with_content]);
    let (_destination, target) = export_file();

    let outcome = export_work(&root, collection_id, &target).expect("the export writes");

    assert!(
        outcome
            .skipped
            .contains(&String::from("“Get user”: 1 saved examples")),
        "the dropped example is named in the report: {:?}",
        outcome.skipped
    );
    assert!(
        outcome
            .skipped
            .contains(&String::from("“Get user”: request settings")),
        "the dropped settings are named in the report: {:?}",
        outcome.skipped
    );
    assert_eq!(
        outcome.skipped.len(),
        2,
        "nothing else below the child level goes unreported: {:?}",
        outcome.skipped
    );
}

#[test]
fn export_flags_literal_credentials_without_their_values() {
    let mut carrying = request(ItemId::new(), "Get user");
    carrying.auth = Auth::Basic {
        username: Some(String::from("demo")),
        password: Some(String::from(LITERAL_TOKEN)),
        extra: BTreeMap::new(),
    };
    carrying.headers = vec![KeyValue {
        key: String::from("Authorization"),
        value: Some(String::from(LITERAL_COOKIE)),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }];
    let mut templated = request(ItemId::new(), "Scoped");
    templated.auth = Auth::Bearer {
        token: Some(String::from("{{api_key}}")),
        extra: BTreeMap::new(),
    };
    let (_workspace, root, collection_id) = seed_workspace(vec![carrying, templated]);
    let (_destination, target) = export_file();

    let outcome = export_work(&root, collection_id, &target).expect("the export writes");
    let message = outcome.message(&target);

    assert!(
        message.contains("contains literal credentials in: “Get user”"),
        "the report names the request carrying literal credentials: {message}"
    );
    assert!(
        !message.contains("Scoped"),
        "a `{{variable}}` credential is not literal and is not named: {message}"
    );
    assert!(
        !message.contains(LITERAL_TOKEN) && !message.contains(LITERAL_COOKIE),
        "the report never carries the values themselves: {message}"
    );
}

#[test]
fn export_replaces_the_target_file_atomically() {
    let (_workspace, root, collection_id) =
        seed_workspace(vec![request(ItemId::new(), "Get user")]);
    let (destination, target) = export_file();
    std::fs::write(&target, "the file that was already there").expect("the old file writes");
    #[cfg(unix)]
    let before = {
        use std::os::unix::fs::MetadataExt as _;
        std::fs::metadata(&target)
            .expect("the old file reads")
            .ino()
    };

    export_work(&root, collection_id, &target).expect("the export writes");

    let exported = std::fs::read_to_string(&target).expect("the export reads back");
    assert!(
        exported.contains("v2.1.0") && !exported.contains("the file that was already there"),
        "the export replaced the file that was there: {exported}"
    );
    let mut left_behind: Vec<String> = std::fs::read_dir(destination.path())
        .expect("the destination folder reads")
        .map(|entry| {
            entry
                .expect("a destination entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left_behind.sort();
    assert_eq!(
        left_behind,
        vec![String::from("users-api.postman_collection.json")],
        "the temp file left nothing behind"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let after = std::fs::metadata(&target).expect("the export reads").ino();
        assert_ne!(
            before, after,
            "truncating in place keeps the file's identity; an atomic replace does not"
        );
    }
}
