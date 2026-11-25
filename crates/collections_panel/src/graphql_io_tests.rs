//! GraphQL requests through the panel's Postman import and export (S50c): a Postman
//! GraphQL item lands as a typed, hardened `Request::GraphQl`, and the export writes it —
//! so it is never listed as skipped, and a credential typed literally into one is named
//! in the report like any HTTP request's (never its value, AGENTS.md › Security).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_model::{Auth, Collection, GraphQlRequest, ItemId, KeyValue, Request};
use serde_json::{Value, json};
use tempfile::TempDir;
use workspace_store::{Item, ItemBody, save_item};

use crate::export::export_work;

use super::import_work;

/// Obviously fake credential values (`.agents/rules/tests.md`).
const LITERAL_TOKEN: &str = "token-REDACTED-example";

fn graphql(name: &str) -> GraphQlRequest {
    GraphQlRequest {
        id: ItemId::new(),
        name: String::from(name),
        url: String::from("https://api.test/graphql"),
        headers: Vec::new(),
        auth: Auth::Inherit,
        query: String::from("{ users { id } }"),
        variables: String::new(),
        operation_name: None,
        settings: None,
        extra: BTreeMap::new(),
    }
}

fn header(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: Some(String::from(value)),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// An empty workspace folder (PRD §6.1).
fn seed_workspace(root: &Path) {
    save_item(
        root,
        &Item {
            body: ItemBody::Workspace(api_model::Workspace {
                id: ItemId::new(),
                name: String::from("GraphQL fixture"),
                format_version: String::from(workspace_store::FORMAT_VERSION),
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::new(),
            path: None,
        },
    )
    .expect("the workspace file writes");
}

/// A workspace holding one collection "Users API" that orders `requests`.
fn seed_collection(requests: Vec<GraphQlRequest>) -> (TempDir, PathBuf, ItemId) {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    seed_workspace(&root);
    let collection_id = ItemId::new();
    save_item(
        &root,
        &Item {
            body: ItemBody::Collection(Collection {
                id: collection_id,
                name: String::from("Users API"),
                description: None,
                auth: Auth::Inherit,
                variables: Vec::new(),
                order: requests.iter().map(|request| request.id).collect(),
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
                body: ItemBody::Request(Request::GraphQl(request)),
                dir: PathBuf::from("collections/users-api"),
                path: None,
            },
        )
        .expect("the request writes");
    }
    (temp, root, collection_id)
}

fn export_target() -> (TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("the export destination folder");
    let target = directory.path().join("users-api.postman_collection.json");
    (directory, target)
}

#[test]
fn test_graphql_request_with_literal_credentials_is_named_in_the_export_warning() {
    let mut with_header = graphql("Header query");
    with_header.headers = vec![header("Authorization", "Bearer token-REDACTED-example")];
    let mut with_auth = graphql("Auth query");
    with_auth.auth = Auth::Bearer {
        token: Some(String::from(LITERAL_TOKEN)),
        extra: BTreeMap::new(),
    };
    let mut templated = graphql("Templated query");
    templated.headers = vec![header("Authorization", "{{token}}")];
    let (_temp, root, id) = seed_collection(vec![with_header, with_auth, templated]);
    let (_directory, target) = export_target();

    let outcome = export_work(&root, id, &target).expect("the export succeeds");

    assert_eq!(
        outcome.literal_credentials,
        vec![String::from("Header query"), String::from("Auth query")],
        "GraphQL requests carrying literal credentials are named, a templated one is not"
    );
    assert!(
        !outcome.message(&target).contains(LITERAL_TOKEN),
        "the report never shows the value"
    );
}

#[test]
fn test_graphql_request_is_exported_and_not_listed_as_skipped() {
    let (_temp, root, id) = seed_collection(vec![graphql("List users")]);
    let (_directory, target) = export_target();

    let outcome = export_work(&root, id, &target).expect("the export succeeds");

    assert!(
        outcome.skipped.is_empty(),
        "a GraphQL request is not skipped: {:?}",
        outcome.skipped
    );
    let bytes = std::fs::read(&target).expect("the export file exists");
    let document: Value = serde_json::from_slice(&bytes).expect("the export is JSON");
    assert_eq!(document["item"][0]["name"], json!("List users"));
    assert_eq!(
        document["item"][0]["request"]["body"]["mode"],
        json!("graphql")
    );
}

#[test]
fn test_postman_graphql_item_imported_through_the_panel_arrives_as_graphql_request() {
    let temp = TempDir::new().expect("a scratch folder");
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(&root).expect("the workspace folder is created");
    seed_workspace(&root);
    let source = temp.path().join("graphql.postman_collection.json");
    let collection = json!({
        "info": {
            "name": "GraphQL API",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": [{
            "name": "List users",
            "query": "planted { field }",
            "request": {
                "method": "POST",
                "header": [{ "key": "Authorization", "value": "Bearer token-REDACTED-example" }],
                "body": {
                    "mode": "graphql",
                    "graphql": { "query": "{ users { id } }", "variables": "{ half typed" }
                },
                "url": "https://api.test/graphql"
            }
        }]
    });
    std::fs::write(
        &source,
        serde_json::to_vec(&collection).expect("the fixture serializes"),
    )
    .expect("the import source writes");

    let outcome = import_work(&root, &source).expect("the import succeeds");

    let snapshot = workspace_store::open(&root).expect("the workspace reopens");
    let imported = snapshot
        .items
        .values()
        .find_map(|item| match &item.body {
            ItemBody::Request(Request::GraphQl(graphql)) => Some(graphql.clone()),
            _ => None,
        })
        .expect("the item lands as a typed GraphQL request");
    assert_eq!(imported.name, "List users");
    assert_eq!(
        imported.query, "{ users { id } }",
        "the file never picks it"
    );
    assert_eq!(imported.variables, "{ half typed");
    assert!(
        outcome.report.problems.iter().any(|problem| {
            problem.path == "List users" && problem.detail.contains("literal credential")
        }),
        "the literal credential is reported by request name: {:?}",
        outcome.report.problems
    );
    assert!(
        outcome
            .report
            .problems
            .iter()
            .all(|problem| !problem.path.contains(LITERAL_TOKEN)
                && !problem.detail.contains(LITERAL_TOKEN)),
        "the report never carries the value"
    );
}
