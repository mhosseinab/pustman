//! Integration tests for the workspace file watcher (REQ-COLLAB-01/02).
//!
//! Wrapped in `mod watcher` so the test filter `watcher` matches exactly these five.

use api_model::{Auth, Body, HttpRequest, ItemId, Method, Request};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use tempfile::TempDir;
use workspace_store::{FsChange, Item, ItemBody, save_item, watch};

#[cfg(test)]
mod watcher {
    use super::*;

    const WAIT: Duration = Duration::from_secs(2);

    const WORKSPACE_JSON: &str = r#"{
  "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "name": "Test workspace",
  "format_version": "1.0"
}"#;

    fn request_json(id: &str, name: &str) -> String {
        format!(
            r#"{{
  "type": "http",
  "id": "{id}",
  "name": "{name}",
  "method": "GET",
  "url": "https://example.com/users/1",
  "headers": []
}}"#
        )
    }

    /// A workspace with one request already on disk.
    fn setup_workspace() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("pustman.json"), WORKSPACE_JSON).expect("write pustman.json");
        std::fs::create_dir_all(root.join("collections/users-api")).expect("mkdir");
        std::fs::write(
            root.join("collections/users-api/get-user.request.json"),
            request_json("01ARZ3NDEKTSV4RRFFQ69G5FB2", "Get user"),
        )
        .expect("write request");
        dir
    }

    fn request_item(name: &str, id: &str) -> Item {
        Item {
            body: ItemBody::Request(Request::Http(HttpRequest {
                id: ItemId::from_str(id).expect("valid id"),
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

    fn expect_timeout(rx: &std::sync::mpsc::Receiver<FsChange>, what: &str) {
        match rx.recv_timeout(WAIT) {
            Err(RecvTimeoutError::Timeout) => {}
            other => panic!("expected no event for {what}, got {other:?}"),
        }
    }

    #[test]
    fn external_add_is_reported_within_one_second() {
        let dir = setup_workspace();
        let root = dir.path();
        let (_watcher, rx) = watch(root).expect("watch");

        let path = root.join("collections/users-api/create-user.request.json");
        std::fs::write(
            &path,
            request_json("01ARZ3NDEKTSV4RRFFQ69G5FB3", "Create user"),
        )
        .expect("write new request");

        let event = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("ItemAdded within 1 s");
        match event {
            FsChange::ItemAdded { id, path: reported } => {
                assert_eq!(
                    id,
                    ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB3").expect("valid id")
                );
                assert_eq!(
                    reported,
                    PathBuf::from("collections/users-api/create-user.request.json")
                );
            }
            other => panic!("expected ItemAdded, got {other:?}"),
        }
    }

    #[test]
    fn external_edit_is_reported() {
        let dir = setup_workspace();
        let root = dir.path();
        let (_watcher, rx) = watch(root).expect("watch");

        // Touch the existing request so the watcher sees a modification.
        let path = root.join("collections/users-api/get-user.request.json");
        std::fs::write(
            &path,
            request_json("01ARZ3NDEKTSV4RRFFQ69G5FB2", "Get user renamed"),
        )
        .expect("rewrite request");

        let event = rx.recv_timeout(WAIT).expect("ItemChanged within 2 s");
        match event {
            FsChange::ItemChanged { id, path: reported } => {
                assert_eq!(
                    id,
                    ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").expect("valid id")
                );
                assert_eq!(
                    reported,
                    PathBuf::from("collections/users-api/get-user.request.json")
                );
            }
            other => panic!("expected ItemChanged, got {other:?}"),
        }

        // A different item now occupies the same path (a git checkout between branches):
        // the old one is gone from here and must be reported as such, or an id-keyed model
        // leaks it.
        std::fs::write(
            &path,
            request_json("01ARZ3NDEKTSV4RRFFQ69G5FB7", "A different item"),
        )
        .expect("rewrite with a different id");
        let old_id = ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB2").expect("valid id");
        let new_id = ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB7").expect("valid id");
        let mut seen = Vec::new();
        while let Ok(event) = rx.recv_timeout(WAIT) {
            let done = matches!(event, FsChange::ItemAdded { .. });
            seen.push(event);
            if done {
                break;
            }
        }
        assert!(
            seen.iter()
                .any(|event| matches!(event, FsChange::ItemRemoved { id, .. } if *id == old_id)),
            "the replaced item is reported removed: {seen:?}"
        );
        assert!(
            seen.iter()
                .any(|event| matches!(event, FsChange::ItemAdded { id, .. } if *id == new_id)),
            "a replacement id is UNSEEN and must arrive as ItemAdded: {seen:?}"
        );
    }

    #[test]
    fn own_save_is_not_reported() {
        let dir = setup_workspace();
        let root = dir.path();
        let (watcher, rx) = watch(root).expect("watch");

        let item = request_item("New request", "01ARZ3NDEKTSV4RRFFQ69G5FB4");
        let saved = save_item(root, &item).expect("save_item");
        watcher.record_own_write(root, &saved);

        expect_timeout(&rx, "own save");
        // The suppression is ONE-SHOT: an external write that restores the very same bytes
        // is a real change (an ABA revert, e.g. a git checkout round-trip) and swallowing it
        // leaves the caller's model stale with no recovery signal.
        let bytes = std::fs::read(root.join(&saved)).expect("read the bytes we saved");
        std::fs::write(root.join(&saved), bytes).expect("external rewrite of the same bytes");
        let event = rx
            .recv_timeout(WAIT)
            .expect("an external revert to our own bytes is reported");
        assert!(
            matches!(
                event,
                FsChange::ItemAdded { .. } | FsChange::ItemChanged { .. }
            ),
            "the external revert must not be suppressed: {event:?}"
        );

        // A rename is ours on BOTH legs. A leaf rename produces no directory event, so the
        // `Rescan` that covers a container rename never reaches it: both paths must be
        // recorded, or the `ItemRemoved` of the old leg tells the caller the item it just
        // renamed is deleted.
        // A different name gives a different slug, so this is a real rename: the item moves
        // from `previous` to a new path and `save_item` deletes the old file.
        let mut renamed = request_item("Renamed request", "01ARZ3NDEKTSV4RRFFQ69G5FB4");
        renamed.path = Some(saved.clone());
        let previous = saved;
        let saved = save_item(root, &renamed).expect("rename save");
        assert_ne!(previous, saved, "the slug changed, so the file moved");
        // The rename and an external write to the old path go back-to-back so they land in
        // the SAME debounce batch. The own-delete marker for `previous` must match only
        // while the path is absent: here it is occupied again by a genuine external change
        // (a git checkout landing in our rename window) and must NOT be eaten.
        watcher.record_own_rename(root, &previous, &saved);
        std::fs::write(
            root.join(&previous),
            request_json("01ARZ3NDEKTSV4RRFFQ69G5FB8", "An external occupant"),
        )
        .expect("external write to the old path");
        let occupant = ItemId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FB8").expect("valid id");
        let mut seen = Vec::new();
        while let Ok(event) = rx.recv_timeout(WAIT) {
            let found = matches!(
                event,
                FsChange::ItemAdded { id, .. } | FsChange::ItemChanged { id, .. } if id == occupant
            );
            seen.push(event);
            if found {
                break;
            }
        }
        assert!(
            seen.iter().any(
                |event| matches!(event, FsChange::ItemAdded { id, .. } | FsChange::ItemChanged { id, .. } if *id == occupant)
            ),
            "a stale own-delete marker must not swallow a genuine external change: {seen:?}"
        );
    }

    #[test]
    fn conflict_markers_reported_as_problem_event() {
        let dir = setup_workspace();
        let root = dir.path();
        let (_watcher, rx) = watch(root).expect("watch");

        let path = root.join("collections/users-api/get-user.request.json");
        std::fs::write(&path, "<<<<<<< HEAD\n{}\n=======\n{}\n>>>>>>> branch\n")
            .expect("write conflicted file");

        let event = rx.recv_timeout(WAIT).expect("Problem within 2 s");
        match event {
            FsChange::Problem(problem) => {
                assert_eq!(
                    problem.path,
                    PathBuf::from("collections/users-api/get-user.request.json")
                );
                assert_eq!(
                    problem.kind,
                    workspace_store::ItemProblemKind::ConflictMarkers
                );
            }
            other => panic!("expected Problem, got {other:?}"),
        }
    }

    #[test]
    fn git_dir_changes_ignored() {
        let dir = setup_workspace();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
        let (_watcher, rx) = watch(root).expect("watch");

        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write .git/HEAD");

        expect_timeout(&rx, ".git change");
    }
}
