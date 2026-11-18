use super::*;
use api_model::{Auth, Body, Environment, HttpRequest, Method, Variable, VariableKind};
use std::fs;
use std::str::FromStr;
use tempfile::TempDir;

/// An obviously fake secret: tests must never carry anything a scanner would trip on.
const FAKE_SECRET: &str = "token-REDACTED-example";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch copy of a fixture workspace, so save tests never write into the repo.
fn copy_fixture(name: &str) -> TempDir {
    let scratch = TempDir::new().unwrap();
    copy_tree(&fixture(name), scratch.path());
    scratch
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn find_by_name<'a>(snapshot: &'a WorkspaceSnapshot, name: &str) -> &'a Item {
    snapshot
        .items
        .values()
        .find(|item| item.name() == name)
        .unwrap_or_else(|| panic!("no item named {name} in {:?}", snapshot.root))
}

fn request_item(name: &str, id: &str) -> Item {
    Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id: ItemId::from_str(id).unwrap(),
            name: name.to_string(),
            method: Method::Get,
            url: "https://example.com".to_string(),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    }
}

#[test]
fn open_reads_sample_workspace() {
    let snapshot = open(fixture("sample").as_path()).unwrap();

    assert!(!snapshot.read_only, "format_version 1.0 opens writable");
    assert!(snapshot.problems.is_empty(), "{:?}", snapshot.problems);
    assert_eq!(snapshot.items.len(), 6, "workspace plus five items");

    let workspace = find_by_name(&snapshot, "Sample workspace");
    assert_eq!(workspace.path, Some(PathBuf::from(WORKSPACE_FILE)));

    let collection = find_by_name(&snapshot, "Users API");
    assert_eq!(
        collection.path,
        Some(PathBuf::from("collections/users-api/collection.json"))
    );

    let folder = find_by_name(&snapshot, "Auth");
    assert_eq!(
        folder.path,
        Some(PathBuf::from("collections/users-api/auth/folder.json"))
    );

    let login = find_by_name(&snapshot, "Login");
    assert_eq!(
        login.path,
        Some(PathBuf::from(
            "collections/users-api/auth/login.request.json"
        ))
    );

    let environment = find_by_name(&snapshot, "Staging");
    assert_eq!(
        environment.path,
        Some(PathBuf::from("environments/staging.environment.json"))
    );

    // The path map is reachable by id, which is what the tree and the tabs use.
    let get_user = find_by_name(&snapshot, "Get user");
    let id = get_user.id().expect("an item file always carries its id");
    assert_eq!(
        snapshot.path(&id),
        Some(Path::new("collections/users-api/get-user.request.json"))
    );
}

#[test]
fn save_is_byte_identical_for_unchanged_item() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();

    for item in snapshot.items.values() {
        let path = item.path.clone().unwrap();
        let before = fs::read(root.join(&path)).unwrap();
        let saved = save_item(root, item).unwrap();
        assert_eq!(saved, path, "{} must not move", item.name());
        let after = fs::read(root.join(&saved)).unwrap();
        assert_eq!(
            before,
            after,
            "saving an unchanged {} must be a zero-byte diff",
            item.name()
        );
    }
}

#[test]
fn save_uses_two_space_sorted_trailing_newline() {
    let scratch = TempDir::new().unwrap();
    // Inserted out of order on purpose: a map serializes sorted.
    let mut extra = BTreeMap::new();
    extra.insert("x-zeta".to_string(), serde_json::json!(2));
    extra.insert("x-alpha".to_string(), serde_json::json!(1));
    let item = Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").unwrap(),
            name: "Get user".to_string(),
            method: Method::Get,
            url: "https://example.com".to_string(),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra,
        })),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    };

    let saved = save_item(scratch.path(), &item).unwrap();
    let text = fs::read_to_string(scratch.path().join(saved)).unwrap();

    assert!(
        text.ends_with("}\n") && !text.ends_with("}\n\n"),
        "exactly one trailing newline: {text:?}"
    );
    assert!(text.contains("\n  \"id\":"), "two-space indent: {text:?}");
    assert_eq!(
        text,
        r#"{
  "type": "http",
  "id": "01ARZ3NDEKTSV4RRFFQ69G5FB2",
  "name": "Get user",
  "method": "GET",
  "url": "https://example.com",
  "x-alpha": 1,
  "x-zeta": 2
}
"#
    );
}

#[test]
fn slug_collision_gets_suffix() {
    let scratch = TempDir::new().unwrap();
    let root = scratch.path();

    let first = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    let second = request_item("get user", "01ARZ3NDEKTSV4RRFFQ69G5FB3");
    let third = request_item("Get-User", "01ARZ3NDEKTSV4RRFFQ69G5FB4");

    // All three names slugify to `get-user`, so only the first keeps it (REQ-WSP-03).
    assert_eq!(
        save_item(root, &first).unwrap(),
        PathBuf::from("collections/users-api/get-user.request.json")
    );
    assert_eq!(
        save_item(root, &second).unwrap(),
        PathBuf::from("collections/users-api/get-user-2.request.json")
    );
    assert_eq!(
        save_item(root, &third).unwrap(),
        PathBuf::from("collections/users-api/get-user-3.request.json")
    );
}

#[test]
fn rename_keeps_id() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let original = find_by_name(&snapshot, "Get user");
    let id = original.id().unwrap();

    let mut renamed = original.clone();
    renamed.set_name("Fetch user");
    let saved = save_item(root, &renamed).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/fetch-user.request.json"),
        "a rename renames the file"
    );
    assert!(
        !root
            .join("collections/users-api/get-user.request.json")
            .exists(),
        "the old file is gone, so Git shows a rename (REQ-WSP-03)"
    );

    let reopened = open(root).unwrap();
    let moved = find_by_name(&reopened, "Fetch user");
    assert_eq!(
        moved.id(),
        Some(id),
        "identity lives in the file, not the name"
    );
    assert_eq!(moved.path, Some(saved));
}

#[test]
fn rename_collection_moves_its_directory() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let original = find_by_name(&snapshot, "Users API");
    let id = original.id().unwrap();

    let mut renamed = original.clone();
    renamed.set_name("People API");
    let saved = save_item(root, &renamed).unwrap();

    // A collection's children live in its `<slug>` directory, so a rename moves the lot.
    assert_eq!(
        saved,
        PathBuf::from("collections/people-api/collection.json")
    );
    assert!(!root.join("collections/users-api").exists());
    assert!(
        root.join("collections/people-api/get-user.request.json")
            .exists(),
        "children come along"
    );

    let reopened = open(root).unwrap();
    let moved = find_by_name(&reopened, "People API");
    assert_eq!(moved.id(), Some(id));
    assert!(
        root.join("collections/people-api/auth/login.request.json")
            .exists(),
        "nested folders come along"
    );
}

#[test]
fn conflict_markers_reported_as_problem() {
    let snapshot = open(fixture("conflict").as_path()).unwrap();

    assert_eq!(
        snapshot.problems,
        vec![ItemProblem {
            path: PathBuf::from("collections/users-api/collection.json"),
            kind: ItemProblemKind::ConflictMarkers,
        }]
    );
    assert!(
        find_by_name(&snapshot, "Get user").id().is_some(),
        "the rest of the workspace still loads (REQ-COLLAB-02)"
    );
}

#[test]
fn open_reports_a_second_file_with_a_duplicate_id() {
    let snapshot = open(fixture("duplicate-id").as_path()).unwrap();

    // A rename/rename merge leaves two files carrying one id. The first in sorted order
    // keeps the id; the other is reported as a problem instead of silently collapsing
    // into it (REQ-COLLAB-02).
    assert_eq!(
        snapshot.problems,
        vec![ItemProblem {
            path: PathBuf::from("collections/users-api/get-user.request.json"),
            kind: ItemProblemKind::DuplicateId {
                id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").unwrap(),
                kept: PathBuf::from("collections/users-api/fetch-user.request.json"),
            },
        }]
    );
    assert_eq!(
        snapshot.items.len(),
        2,
        "workspace plus one request, not two"
    );

    let survivor = find_by_name(&snapshot, "Fetch user");
    assert_eq!(
        survivor.id().unwrap(),
        ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").unwrap()
    );
    assert_eq!(
        survivor.path,
        Some(PathBuf::from(
            "collections/users-api/fetch-user.request.json"
        )),
        "the first file in sorted order keeps the id"
    );
}

#[test]
fn renaming_onto_a_duplicate_id_sibling_keeps_both_files() {
    let scratch = copy_fixture("duplicate-id");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let survivor = find_by_name(&snapshot, "Fetch user").clone();
    let hidden = PathBuf::from("collections/users-api/get-user.request.json");
    let hidden_bytes = fs::read(root.join(&hidden)).unwrap();

    // The hidden sibling shares the survivor's id but carries its own content, so it is
    // another file, not this item in another spelling: renaming the survivor onto its
    // name takes a suffixed name and leaves the sibling's bytes alone (REQ-COLLAB-02).
    let mut renamed = survivor;
    renamed.set_name("Get user");
    let saved = save_item(root, &renamed).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/get-user-2.request.json"),
        "the sibling's name is taken, so the survivor is suffixed"
    );
    assert_eq!(
        fs::read(root.join(&hidden)).unwrap(),
        hidden_bytes,
        "the sibling keeps its distinct content"
    );
    assert!(
        !root
            .join("collections/users-api/fetch-user.request.json")
            .exists(),
        "the survivor's own old file is still renamed away"
    );

    let reopened = open(root).unwrap();
    assert_eq!(
        reopened.problems,
        vec![ItemProblem {
            path: hidden,
            kind: ItemProblemKind::DuplicateId {
                id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").unwrap(),
                kept: PathBuf::from("collections/users-api/get-user-2.request.json"),
            },
        }],
        "both halves of the merge are still on disk and still visible"
    );
}

#[test]
fn invalid_json_reported_with_position() {
    let snapshot = open(fixture("invalid-json").as_path()).unwrap();

    assert_eq!(
        snapshot.problems,
        vec![ItemProblem {
            path: PathBuf::from("collections/users-api/get-user.request.json"),
            // The missing comma after `"Get user"` is caught where the next token starts,
            // and serde_json's own message counts that as line 5, column 3 ("at line 5
            // column 3") — its `line()` and `column()` are already 1-based and are the
            // numbers shown to the user, never one higher (REQ-COLLAB-02).
            kind: ItemProblemKind::InvalidJson { line: 5, col: 3 },
        }]
    );
}

#[test]
fn invalid_json_position_is_serde_jsons_own_position() {
    let snapshot = open(fixture("invalid-json").as_path()).unwrap();
    let problem = snapshot
        .problems
        .first()
        .expect("the fixture does not parse");
    let (line, col) = match &problem.kind {
        ItemProblemKind::InvalidJson { line, col } => (*line, *col),
        other => panic!("expected InvalidJson, got {other:?}"),
    };

    let bytes =
        fs::read(fixture("invalid-json").join("collections/users-api/get-user.request.json"))
            .unwrap();
    let error = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_err();

    // serde_json's `line()` and `column()` are 1-based and its own message prints those
    // same numbers, so the report is them verbatim: adding 1 to either points past the
    // offending character.
    assert_eq!((line, col), (5, 3), "the position as serde_json counts it");
    assert_eq!(
        (line, col),
        (error.line(), error.column()),
        "no fudge between the parse error and the report"
    );
}

#[test]
fn newer_major_version_opens_read_only() {
    let snapshot = open(fixture("format-v2").as_path()).unwrap();
    assert!(snapshot.read_only, "format_version 2.0 is newer than 1.x");

    let snapshot = open(fixture("sample").as_path()).unwrap();
    assert!(!snapshot.read_only, "format_version 1.0 is this build's");
}

#[test]
fn secret_variable_file_has_no_value() {
    let scratch = TempDir::new().unwrap();
    let item = Item {
        body: ItemBody::Environment(Environment {
            id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB4").unwrap(),
            name: "Staging".to_string(),
            variables: vec![Variable {
                key: "api_key".to_string(),
                value: Some(FAKE_SECRET.to_string()),
                kind: VariableKind::Secret,
                enabled: true,
                description: Some("team key".to_string()),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from(ENVIRONMENTS_DIR),
        path: None,
    };

    let saved = save_item(scratch.path(), &item).unwrap();
    let text = fs::read_to_string(scratch.path().join(saved)).unwrap();

    assert!(
        !text.contains(FAKE_SECRET),
        "a secret value must never reach a file (REQ-VAR-SHARE-02)"
    );
    assert!(!text.contains("\"value\""), "no value field at all");
    let written: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        written["variables"][0],
        serde_json::json!({
            "key": "api_key",
            "kind": "secret",
            "description": "team key"
        }),
        "the file records only key, kind and description"
    );
}

#[test]
fn save_refuses_an_item_into_a_newer_format_workspace() {
    let scratch = copy_fixture("format-v2");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    assert!(snapshot.read_only, "format_version 2.0 opens read-only");

    // `read_only` is not a banner to be ignored: a 1.x save into a 2.0 workspace leaves
    // the newer reader files it never asked for (REQ-WSP-04).
    let refused = save_item(
        root,
        &request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2"),
    )
    .unwrap_err();
    match &refused {
        SaveError::ReadOnly(version) => {
            assert_eq!(
                version.as_str(),
                "2.0",
                "the declared version is what is refused"
            )
        }
        other => panic!("expected ReadOnly, got {other:?}"),
    }
    assert!(
        !root.join("collections").exists(),
        "nothing at all is created in a read-only workspace"
    );
    assert_eq!(
        fs::read_to_string(root.join(WORKSPACE_FILE)).unwrap(),
        fs::read_to_string(fixture("format-v2").join(WORKSPACE_FILE)).unwrap(),
        "pustman.json itself is untouched"
    );

    let workspace = find_by_name(&snapshot, "Workspace written by a newer Pustman");
    assert!(
        matches!(save_item(root, workspace), Err(SaveError::ReadOnly(_))),
        "rewriting pustman.json is refused too"
    );
}

#[test]
fn save_refuses_an_item_that_would_leave_the_workspace() {
    let sandbox = TempDir::new().unwrap();
    let root = sandbox.path().join("workspace");
    copy_tree(&fixture("sample"), &root);
    let escape = sandbox.path().join("escape");
    fs::create_dir(&escape).unwrap();
    fs::write(escape.join("old.request.json"), "keep me").unwrap();

    // `Item::dir` is a write base and `Item::path` is the file a rename removes, so both
    // are relative to the root and stay in it: a climb out and an absolute anchor are
    // refused before the first write.
    let mut climbing = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    climbing.dir = PathBuf::from("../escape");
    let refused = save_item(&root, &climbing).unwrap_err();
    assert!(matches!(refused, SaveError::OutsideRoot(_)), "{refused:?}");

    let mut absolute = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    absolute.dir = escape.clone();
    let refused = save_item(&root, &absolute).unwrap_err();
    assert!(matches!(refused, SaveError::OutsideRoot(_)), "{refused:?}");

    let mut removing = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    removing.path = Some(PathBuf::from("../escape/old.request.json"));
    let refused = save_item(&root, &removing).unwrap_err();
    assert!(matches!(refused, SaveError::OutsideRoot(_)), "{refused:?}");

    assert_eq!(
        fs::read_to_string(escape.join("old.request.json")).unwrap(),
        "keep me",
        "a save never reaches outside the workspace"
    );
    assert_eq!(
        fs::read_dir(&escape).unwrap().count(),
        1,
        "nothing new appears next to the workspace"
    );
    assert!(
        !root
            .join("collections/users-api/get-user-2.request.json")
            .exists(),
        "and nothing was written inside either"
    );
}

#[cfg(unix)]
#[test]
fn save_refuses_to_follow_a_symlinked_directory() {
    use std::os::unix::fs::symlink;

    let sandbox = TempDir::new().unwrap();
    let root = sandbox.path().join("workspace");
    copy_tree(&fixture("sample"), &root);
    let outside = sandbox.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("old.request.json"), "keep me").unwrap();
    symlink(&outside, root.join("collections/linked")).unwrap();

    // A directory symlink inside an untrusted workspace points a write at a folder of the
    // workspace's choosing, so neither writing into it nor removing through it is allowed.
    let mut writing = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    writing.dir = PathBuf::from("collections/linked");
    let refused = save_item(&root, &writing).unwrap_err();
    assert!(matches!(refused, SaveError::OutsideRoot(_)), "{refused:?}");

    let mut removing = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    removing.path = Some(PathBuf::from("collections/linked/old.request.json"));
    let refused = save_item(&root, &removing).unwrap_err();
    assert!(matches!(refused, SaveError::OutsideRoot(_)), "{refused:?}");

    assert_eq!(
        fs::read_to_string(outside.join("old.request.json")).unwrap(),
        "keep me",
        "a save never follows the symlink out"
    );
    assert_eq!(
        fs::read_dir(&outside).unwrap().count(),
        1,
        "nothing new appears outside"
    );
    assert!(
        !root
            .join("collections/users-api/get-user-2.request.json")
            .exists(),
        "and nothing was written inside either"
    );
}

#[cfg(unix)]
#[test]
fn open_does_not_follow_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let sandbox = TempDir::new().unwrap();
    let root = sandbox.path().join("workspace");
    copy_tree(&fixture("sample"), &root);
    // A cycle back into the workspace and a way out of it: following either reads a tree
    // that is not this workspace, and the cycle goes round for ever.
    symlink(&root, root.join("collections/loop")).unwrap();
    let outside = sandbox.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(
        outside.join("steal.request.json"),
        r#"{ "type": "http", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB5", "name": "Steal", "method": "GET", "url": "https://example.com" }"#,
    )
    .unwrap();
    symlink(&outside, root.join("collections/linked")).unwrap();

    let snapshot = open(&root).unwrap();

    assert!(snapshot.problems.is_empty(), "{:?}", snapshot.problems);
    assert_eq!(
        snapshot.items.len(),
        6,
        "the workspace's own items, and no more: {:?}",
        snapshot.items.values().map(Item::name).collect::<Vec<_>>()
    );
    assert!(
        snapshot.items.values().all(|item| item.name() != "Steal"),
        "nothing is read through a directory symlink"
    );
}

/// A different item's file, as an external edit (git checkout, another clone) leaves it at
/// a path this build's snapshot still knows.
const FOREIGN_REQUEST: &str = r#"{ "type": "http", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB5", "name": "Someone else", "method": "GET", "url": "https://example.com/other" }"#;

#[test]
fn saving_a_child_after_its_folder_moved_follows_the_move() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mut folder = find_by_name(&snapshot, "Auth").clone();
    // The child is read before the move, which is exactly what the app holds after the
    // user renames its folder and keeps editing it.
    let mut login = find_by_name(&snapshot, "Login").clone();

    folder.set_name("Security");
    let moved = save_item(root, &folder).unwrap();
    assert_eq!(
        moved,
        PathBuf::from("collections/users-api/security/folder.json"),
        "renaming the folder moves its directory"
    );

    match &mut login.body {
        ItemBody::Request(Request::Http(request)) => {
            request.url = "https://example.com/signin".to_string();
        }
        other => panic!("expected an http request, got {other:?}"),
    }
    let saved = save_item(root, &login).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/security/login.request.json"),
        "the child follows its folder's move instead of naming the old place"
    );
    assert!(
        !root.join("collections/users-api/auth").exists(),
        "the old folder is not rebuilt"
    );
    let text = fs::read_to_string(root.join(&saved)).unwrap();
    assert!(
        text.contains("https://example.com/signin"),
        "the edit goes with the file: {text}"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        find_by_name(&reopened, "Login").path,
        Some(saved),
        "one file carries the child's id, where its folder now lives"
    );
}

#[test]
fn save_refuses_a_child_when_several_files_carry_its_id() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mut folder = find_by_name(&snapshot, "Auth").clone();
    let mut login = find_by_name(&snapshot, "Login").clone();

    folder.set_name("Security");
    save_item(root, &folder).unwrap();

    // A rename/rename merge left a second file carrying the child's id beside the move, so
    // which file the child moved to cannot be told apart.
    let twin = PathBuf::from("collections/users-api/extra/login.request.json");
    fs::create_dir_all(root.join("collections/users-api/extra")).unwrap();
    let twin_json = r#"{ "type": "http", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB3", "name": "Login twin", "method": "GET", "url": "https://example.com/twin" }"#;
    fs::write(root.join(&twin), twin_json).unwrap();
    let moved = PathBuf::from("collections/users-api/security/login.request.json");
    let moved_bytes = fs::read(root.join(&moved)).unwrap();

    match &mut login.body {
        ItemBody::Request(Request::Http(request)) => {
            request.url = "https://example.com/signin".to_string();
        }
        other => panic!("expected an http request, got {other:?}"),
    }
    save_item(root, &login)
        .expect_err("two files carry the id, so the save has no one home to follow");

    // Writing anywhere would pick a winner among the merge's halves: every file is left as
    // it is (REQ-COLLAB-02).
    assert!(
        !root.join("collections/users-api/auth").exists(),
        "nothing is rebuilt at the old place"
    );
    assert_eq!(fs::read(root.join(&moved)).unwrap(), moved_bytes);
    assert_eq!(
        fs::read(root.join(&twin)).unwrap(),
        twin_json.as_bytes(),
        "the merge's other half keeps its own content"
    );
    assert_eq!(
        fs::read_dir(root.join("collections/users-api/extra"))
            .unwrap()
            .count(),
        1,
        "and no third file appears"
    );

    let reopened = open(root).unwrap();
    assert_eq!(
        reopened.problems.len(),
        1,
        "both halves are still on disk and still reported: {:?}",
        reopened.problems
    );
}

#[test]
fn saving_over_a_foreign_file_takes_a_suffix_instead_of_clobbering_it() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mine = find_by_name(&snapshot, "Get user").clone();

    let foreign = PathBuf::from("collections/users-api/get-user.request.json");
    fs::write(root.join(&foreign), FOREIGN_REQUEST).unwrap();

    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/get-user-2.request.json"),
        "the name is another item's file now, so this one is suffixed (REQ-WSP-03)"
    );
    assert_eq!(
        fs::read(root.join(&foreign)).unwrap(),
        FOREIGN_REQUEST.as_bytes(),
        "the other item keeps its file (REQ-COLLAB-02)"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        find_by_name(&reopened, "Someone else").path,
        Some(foreign),
        "the other item keeps its name and its place"
    );
    let again = find_by_name(&reopened, "Get user");
    assert_eq!(
        again.id(),
        mine.id(),
        "identity lives in the file, not the name"
    );
    assert_eq!(again.path, Some(saved), "and keeps its own new file");
}

#[test]
fn renaming_away_from_a_foreign_file_leaves_it_alone() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mut mine = find_by_name(&snapshot, "Get user").clone();

    let foreign = PathBuf::from("collections/users-api/get-user.request.json");
    fs::write(root.join(&foreign), FOREIGN_REQUEST).unwrap();

    mine.set_name("Fetch user");
    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/fetch-user.request.json")
    );
    assert_eq!(
        fs::read(root.join(&foreign)).unwrap(),
        FOREIGN_REQUEST.as_bytes(),
        "a rename removes the item's own old file only, never the other item's"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        find_by_name(&reopened, "Someone else").path,
        Some(foreign),
        "the other item is untouched"
    );
}

#[test]
fn saving_a_collection_after_its_children_adopts_their_directory() {
    let scratch = TempDir::new().unwrap();
    let root = scratch.path();
    let workspace = Item {
        body: ItemBody::Workspace(Workspace {
            id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB9").unwrap(),
            name: "Fresh workspace".to_string(),
            format_version: FORMAT_VERSION.to_string(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::new(),
        path: None,
    };
    save_item(root, &workspace).unwrap();

    // Autosave and import write children before their container has ever been saved.
    let request = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    save_item(root, &request).unwrap();

    let collection = Item {
        body: ItemBody::Collection(Collection {
            id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap(),
            name: "Users API".to_string(),
            description: None,
            auth: Auth::Inherit,
            variables: Vec::new(),
            order: vec![ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").unwrap()],
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from(COLLECTIONS_DIR),
        path: None,
    };

    let saved = save_item(root, &collection).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/collection.json"),
        "the collection adopts the directory its children are in (PRD §6.1)"
    );
    assert!(
        !root.join("collections/users-api-2").exists(),
        "children are addressed by `order` ids, so the directory is free to adopt"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        reopened.items.len(),
        3,
        "workspace, collection and the request inside it: {:?}",
        reopened.items.values().map(Item::name).collect::<Vec<_>>()
    );
}

/// Another item's collection file, left at a path this build's snapshot still knows.
const FOREIGN_COLLECTION: &str =
    r#"{ "id": "01ARZ3NDEKTSV4RRFFQ69G5FB5", "name": "Foreign collection" }"#;

/// Another item's folder file, left at a path this build's snapshot still knows.
const FOREIGN_FOLDER: &str = r#"{ "id": "01ARZ3NDEKTSV4RRFFQ69G5FB5", "name": "Foreign folder" }"#;

/// Both halves of a merge conflict in one file: not JSON, so no `id` can be read out of it,
/// and both halves of the user's work are in it (REQ-COLLAB-02).
const CONFLICTED_REQUEST: &str = r#"<<<<<<< HEAD
{ "type": "http", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB2", "name": "Get user", "method": "GET", "url": "https://example.com/ours" }
=======
{ "type": "http", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB2", "name": "Get user", "method": "GET", "url": "https://example.com/theirs" }
>>>>>>> feature/users
"#;

/// A request in a directory its container has not been saved into yet.
const ORPHAN_REQUEST: &str = r#"{ "type": "http", "id": "01ARZ3NDEKTSV4RRFFQ69G5FB6", "name": "Orphan", "method": "GET", "url": "https://example.com/orphan" }"#;

#[test]
fn saving_a_collection_over_a_foreign_file_takes_a_suffix_instead_of_clobbering_it() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mine = find_by_name(&snapshot, "Users API").clone();

    let foreign = PathBuf::from("collections/users-api/collection.json");
    fs::write(root.join(&foreign), FOREIGN_COLLECTION).unwrap();

    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api-2/collection.json"),
        "the name is another item's file now, so this one is suffixed (REQ-WSP-03)"
    );
    assert_eq!(
        fs::read(root.join(&foreign)).unwrap(),
        FOREIGN_COLLECTION.as_bytes(),
        "the other item keeps its file: the directory move must not carry it off to be overwritten (REQ-COLLAB-02)"
    );
    assert!(
        root.join("collections/users-api/get-user.request.json")
            .exists(),
        "the directory is left where it is, children and all"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        find_by_name(&reopened, "Foreign collection").path,
        Some(foreign),
        "the other item keeps its name and its place"
    );
    let again = find_by_name(&reopened, "Users API");
    assert_eq!(
        again.id(),
        mine.id(),
        "identity lives in the file, not the name"
    );
    assert_eq!(again.path, Some(saved), "and keeps its own new file");
}

#[test]
fn saving_a_folder_over_a_foreign_file_takes_a_suffix_instead_of_clobbering_it() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mine = find_by_name(&snapshot, "Auth").clone();

    let foreign = PathBuf::from("collections/users-api/auth/folder.json");
    fs::write(root.join(&foreign), FOREIGN_FOLDER).unwrap();

    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/auth-2/folder.json"),
        "the name is another item's file now, so this one is suffixed (REQ-WSP-03)"
    );
    assert_eq!(
        fs::read(root.join(&foreign)).unwrap(),
        FOREIGN_FOLDER.as_bytes(),
        "the other item keeps its file: the directory move must not carry it off to be overwritten (REQ-COLLAB-02)"
    );
    assert!(
        root.join("collections/users-api/auth/login.request.json")
            .exists(),
        "the directory is left where it is, children and all"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        find_by_name(&reopened, "Foreign folder").path,
        Some(foreign),
        "the other item keeps its name and its place"
    );
    let again = find_by_name(&reopened, "Auth");
    assert_eq!(
        again.id(),
        mine.id(),
        "identity lives in the file, not the name"
    );
    assert_eq!(again.path, Some(saved), "and keeps its own new file");
}

#[test]
fn a_conflicted_file_at_the_items_own_path_is_never_overwritten() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mine = find_by_name(&snapshot, "Get user").clone();

    let conflicted = PathBuf::from("collections/users-api/get-user.request.json");
    fs::write(root.join(&conflicted), CONFLICTED_REQUEST).unwrap();

    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/get-user-2.request.json"),
        "the item's own name holds a problem file now, so the item is suffixed"
    );
    assert_eq!(
        fs::read(root.join(&conflicted)).unwrap(),
        CONFLICTED_REQUEST.as_bytes(),
        "both merge halves survive: problem files are never overwritten (REQ-COLLAB-02)"
    );

    let reopened = open(root).unwrap();
    assert_eq!(
        reopened.problems,
        vec![ItemProblem {
            path: conflicted,
            kind: ItemProblemKind::ConflictMarkers,
        }]
    );
    assert_eq!(
        find_by_name(&reopened, "Get user").path,
        Some(saved),
        "the item keeps its id in its own new file"
    );
}

#[test]
fn a_secret_under_a_variable_key_is_scrubbed_from_an_unsupported_request() {
    let scratch = TempDir::new().unwrap();
    let item = Item {
        body: ItemBody::Request(Request::Unsupported(serde_json::json!({
            "type": "graphql",
            "id": "01ARZ3NDEKTSV4RRFFQ69G5FB6",
            "name": "Search",
            "variable": [
                { "key": "api_key", "kind": "secret", "value": FAKE_SECRET, "description": "team key" }
            ]
        }))),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    };

    let saved = save_item(scratch.path(), &item).unwrap();
    let text = fs::read_to_string(scratch.path().join(saved)).unwrap();

    assert!(
        !text.contains(FAKE_SECRET),
        "a secret value must never reach a file (REQ-VAR-SHARE-02)"
    );
    let written: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        written["variable"][0],
        serde_json::json!({
            "key": "api_key",
            "kind": "secret",
            "description": "team key"
        }),
        "the file records only key, kind and description, wherever the variable sits"
    );
    assert_eq!(written["name"], "Search", "the item itself is untouched");
}

#[test]
fn a_secret_under_an_extra_key_is_scrubbed_from_an_environment() {
    let scratch = TempDir::new().unwrap();
    let mut extra = BTreeMap::new();
    extra.insert(
        "vars".to_string(),
        serde_json::json!([
            { "key": "api_key", "kind": "secret", "value": FAKE_SECRET, "description": "team key" }
        ]),
    );
    let item = Item {
        body: ItemBody::Environment(Environment {
            id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB4").unwrap(),
            name: "Staging".to_string(),
            variables: Vec::new(),
            extra,
        }),
        dir: PathBuf::from(ENVIRONMENTS_DIR),
        path: None,
    };

    let saved = save_item(scratch.path(), &item).unwrap();
    let text = fs::read_to_string(scratch.path().join(saved)).unwrap();

    assert!(
        !text.contains(FAKE_SECRET),
        "a secret value must never reach a file (REQ-VAR-SHARE-02)"
    );
    let written: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        written["vars"][0],
        serde_json::json!({
            "key": "api_key",
            "kind": "secret",
            "description": "team key"
        }),
        "the file records only key, kind and description, wherever the variable sits"
    );
    assert_eq!(written["name"], "Staging", "the item itself is untouched");
}

#[test]
fn renaming_a_container_onto_an_adoptable_directory_writes_into_it() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mut mine = find_by_name(&snapshot, "Users API").clone();

    // A directory an earlier save left behind: children in it and no container file, so the
    // name is free and the collection takes the directory together with them (PRD §6.1).
    let orphan = PathBuf::from("collections/people-api/orphan.request.json");
    fs::create_dir_all(root.join("collections/people-api")).unwrap();
    fs::write(root.join(&orphan), ORPHAN_REQUEST).unwrap();

    mine.set_name("People API");
    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/people-api/collection.json"),
        "the adoptable directory holds the free name, so the save lands in it"
    );
    assert_eq!(
        fs::read(root.join(&orphan)).unwrap(),
        ORPHAN_REQUEST.as_bytes(),
        "the children already in it are adopted, never overwritten (PRD §6.1)"
    );
    assert!(
        root.join("collections/users-api/get-user.request.json")
            .exists(),
        "nothing in the old directory is lost"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        find_by_name(&reopened, "People API").path,
        Some(saved),
        "and the collection is where the name says it is"
    );
}

#[test]
fn saving_over_its_own_interrupted_rename_heals_the_pair() {
    let scratch = copy_fixture("sample");
    let root = scratch.path();
    let snapshot = open(root).unwrap();
    let mut mine = find_by_name(&snapshot, "Get user").clone();

    // A crash after a rename's new file was persisted and before the old one was removed:
    // two files carry the id and the bytes are the same, so this is the item's own save
    // half-done and not another file sharing its id (REQ-COLLAB-02).
    mine.set_name("Fetch user");
    let artifact = mine.to_bytes().unwrap();
    fs::write(
        root.join("collections/users-api/fetch-user.request.json"),
        &artifact,
    )
    .unwrap();

    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/fetch-user.request.json"),
        "the interrupted rename is this item's own file, so it keeps the name"
    );
    assert_eq!(
        fs::read(root.join(&saved)).unwrap(),
        artifact,
        "and the save is the byte-identical no-op"
    );
    assert!(
        !root
            .join("collections/users-api/get-user.request.json")
            .exists(),
        "the cleanup takes the old file"
    );

    let reopened = open(root).unwrap();
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(
        find_by_name(&reopened, "Fetch user").path,
        Some(saved),
        "one file carries the id, not a duplicate pair"
    );
}

#[test]
fn a_moved_file_of_another_kind_is_not_this_items_home() {
    let scratch = TempDir::new().unwrap();
    let root = scratch.path();
    let workspace = Item {
        body: ItemBody::Workspace(Workspace {
            id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            name: "Fresh workspace".to_string(),
            format_version: FORMAT_VERSION.to_string(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::new(),
        path: None,
    };
    save_item(root, &workspace).unwrap();

    let mut mine = request_item("Get user", "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    let written = save_item(root, &mine).unwrap();
    mine.path = Some(written.clone());

    // The request's directory goes away and an environment is left squatting on its id: a
    // file is this item's home only where its kind matches the body's, or the save would
    // delete the other item and land where `classify` never reads it (REQ-COLLAB-02).
    let twin = PathBuf::from("environments/twin.environment.json");
    let twin_json = r#"{ "id": "01ARZ3NDEKTSV4RRFFQ69G5FB2", "name": "Twin", "variables": [] }
"#;
    fs::create_dir_all(root.join(ENVIRONMENTS_DIR)).unwrap();
    fs::write(root.join(&twin), twin_json).unwrap();
    fs::remove_dir_all(root.join("collections/users-api")).unwrap();

    let saved = save_item(root, &mine).unwrap();

    assert_eq!(
        saved,
        PathBuf::from("collections/users-api/get-user.request.json"),
        "an environment is not a request's home, so the save stands where the item thinks it is"
    );
    assert_eq!(
        fs::read(root.join(&twin)).unwrap(),
        twin_json.as_bytes(),
        "the other item's file is never deleted (REQ-COLLAB-02)"
    );
    assert!(
        !root.join("environments/get-user.request.json").exists(),
        "and the request never lands where a request is never read (REQ-WSP-01)"
    );

    let reopened = open(root).unwrap();
    assert_eq!(
        find_by_name(&reopened, "Get user").path,
        Some(written),
        "the request is still an item where its kind is read"
    );
    assert_eq!(
        reopened.problems,
        vec![ItemProblem {
            path: twin,
            kind: ItemProblemKind::DuplicateId {
                id: ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").unwrap(),
                kept: saved,
            },
        }],
        "the id both files carry is reported, with neither file gone"
    );
}
