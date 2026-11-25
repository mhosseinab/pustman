use super::*;
use crate::{Item, ItemBody, open, save_item};
use api_model::{ItemId, Request};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tempfile::TempDir;

const WORKSPACE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// One request of every kind, decoded from the JSON shape its file carries, with the
/// suffix that kind is saved under.
fn every_kind() -> Vec<(Request, &'static str)> {
    let decode = |value: serde_json::Value| -> Request { serde_json::from_value(value).unwrap() };
    vec![
        (
            decode(serde_json::json!({
                "type": "http", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB1", "name": "Get user",
                "method": "GET", "url": "https://api.example.test/users/1"
            })),
            ".request.json",
        ),
        (
            decode(serde_json::json!({
                "type": "graphql", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB2", "name": "Search",
                "url": "https://api.example.test/graphql", "query": "{ users { id } }"
            })),
            ".graphql.json",
        ),
        (
            decode(serde_json::json!({
                "type": "websocket", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB3", "name": "Chat",
                "url": "wss://echo.example.test/ws"
            })),
            ".websocket.json",
        ),
        (
            decode(serde_json::json!({
                "type": "socketio", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB4", "name": "Live feed",
                "url": "https://io.example.test"
            })),
            ".socketio.json",
        ),
        (
            decode(serde_json::json!({
                "type": "grpc", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB5", "name": "Greeter",
                "target": "localhost:50051"
            })),
            ".grpc.json",
        ),
    ]
}

fn request_item(request: Request) -> Item {
    Item {
        body: ItemBody::Request(request),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    }
}

fn write_workspace(root: &Path) {
    fs::write(
        root.join("pustman.json"),
        format!(
            "{{\n  \"id\": \"{WORKSPACE_ID}\",\n  \"name\": \"Kinds\",\n  \"format_version\": \"1.0\"\n}}\n"
        ),
    )
    .unwrap();
}

#[test]
fn each_kind_saves_under_its_suffix() {
    let scratch = TempDir::new().unwrap();
    for (request, suffix) in every_kind() {
        assert_eq!(suffix_for(&request), suffix, "{}", request.type_tag());
        let slug = crate::slugify(request.name());
        let saved = save_item(scratch.path(), &request_item(request.clone())).unwrap();
        assert_eq!(
            saved,
            PathBuf::from(format!("collections/users-api/{slug}{suffix}")),
            "a {} request is written under its own suffix",
            request.type_tag()
        );
        let bytes = fs::read(scratch.path().join(&saved)).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            text.ends_with("}\n"),
            "deterministic JSON ends in a newline"
        );
        let decoded: Request = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            decoded, request,
            "the file decodes back to the same request"
        );

        let again = save_item(
            scratch.path(),
            &Item {
                path: Some(saved.clone()),
                ..request_item(request)
            },
        )
        .unwrap();
        assert_eq!(again, saved, "saving again keeps the same file");
        assert_eq!(
            fs::read(scratch.path().join(&saved)).unwrap(),
            bytes,
            "an unchanged save is byte-identical (REQ-WSP-02)"
        );
    }
}

#[test]
fn snapshot_loads_every_kind_from_disk() {
    let scratch = TempDir::new().unwrap();
    let root = scratch.path();
    write_workspace(root);
    let mut expected = Vec::new();
    for (request, suffix) in every_kind() {
        let relative = PathBuf::from(format!(
            "collections/users-api/{}{suffix}",
            crate::slugify(request.name())
        ));
        fs::create_dir_all(root.join("collections/users-api")).unwrap();
        fs::write(root.join(&relative), serde_json::to_vec(&request).unwrap()).unwrap();
        expected.push((request, relative));
    }

    let snapshot = open(root).unwrap();

    assert!(
        snapshot.problems.is_empty(),
        "every kind's file is an item: {:?}",
        snapshot.problems
    );
    for (request, relative) in expected {
        let id = request.id().unwrap();
        let item = snapshot
            .items
            .get(&id)
            .unwrap_or_else(|| panic!("{} is missing from the snapshot", relative.display()));
        assert_eq!(item.body, ItemBody::Request(request));
        assert_eq!(item.path.as_deref(), Some(relative.as_path()));
        assert_eq!(item.dir, PathBuf::from("collections/users-api"));
    }
}

#[test]
fn unknown_suffix_is_not_a_request() {
    assert_eq!(
        kind_for_file_name("get-user.request.json"),
        Some(RequestKind::Http)
    );
    assert_eq!(
        kind_for_file_name("search.graphql.json"),
        Some(RequestKind::GraphQl)
    );
    assert_eq!(
        kind_for_file_name("chat.websocket.json"),
        Some(RequestKind::WebSocket)
    );
    assert_eq!(
        kind_for_file_name("feed.socketio.json"),
        Some(RequestKind::SocketIo)
    );
    assert_eq!(
        kind_for_file_name("greeter.grpc.json"),
        Some(RequestKind::Grpc)
    );
    for name in [
        "broker.mqtt.json",
        "notes.json",
        "graphql.json",
        "search.graphql.json.bak",
        "search.GRAPHQL.json",
        "collection.json",
    ] {
        assert_eq!(
            kind_for_file_name(name),
            None,
            "{name} is not a request file"
        );
    }

    let scratch = TempDir::new().unwrap();
    let root = scratch.path();
    write_workspace(root);
    fs::create_dir_all(root.join("collections/users-api")).unwrap();
    fs::write(
        root.join("collections/users-api/broker.mqtt.json"),
        r#"{"type":"mqtt","id":"01ARZ3NDEKTSV4RRFFQ69G5FB7","name":"Broker"}"#,
    )
    .unwrap();

    let snapshot = open(root).unwrap();
    assert_eq!(
        snapshot.items.len(),
        1,
        "only the workspace itself is an item"
    );
    assert!(
        snapshot.problems.is_empty(),
        "an unknown file is ignored, not a problem"
    );
}

#[test]
fn changing_kind_renames_the_file() {
    let scratch = TempDir::new().unwrap();
    let root = scratch.path();
    write_workspace(root);
    let id = "01ARZ3NDEKTSV4RRFFQ69G5FB3";
    let websocket: Request = serde_json::from_value(serde_json::json!({
        "type": "websocket", "id": id, "name": "Chat", "url": "wss://echo.example.test/ws"
    }))
    .unwrap();
    let first = save_item(root, &request_item(websocket)).unwrap();
    assert_eq!(
        first,
        PathBuf::from("collections/users-api/chat.websocket.json")
    );

    let graphql: Request = serde_json::from_value(serde_json::json!({
        "type": "graphql", "id": id, "name": "Chat", "url": "https://api.example.test/graphql"
    }))
    .unwrap();
    let moved = save_item(
        root,
        &Item {
            path: Some(first.clone()),
            ..request_item(graphql.clone())
        },
    )
    .unwrap();

    assert_eq!(
        moved,
        PathBuf::from("collections/users-api/chat.graphql.json")
    );
    assert!(!root.join(&first).exists(), "the old kind's file is gone");
    let snapshot = open(root).unwrap();
    assert!(snapshot.problems.is_empty(), "{:?}", snapshot.problems);
    let item = snapshot.items.get(&ItemId::from_str(id).unwrap()).unwrap();
    assert_eq!(
        item.body,
        ItemBody::Request(graphql),
        "the id survives the move"
    );
    assert_eq!(item.path.as_deref(), Some(moved.as_path()));
}
