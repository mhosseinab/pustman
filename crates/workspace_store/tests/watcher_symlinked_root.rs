//! A workspace opened through a symlinked path (macOS keeps temp dirs under the `/var` ->
//! `/private/var` symlink, and FSEvents reports the resolved path) must still report
//! external changes. On Linux, inotify reports the path as watched, so this pins only that
//! watching through a symlink keeps working; macOS CI's `watcher` tests are the proof for
//! the resolved-path case.

#![allow(clippy::expect_used)]

use std::time::Duration;
use workspace_store::{FsChange, watch};

#[cfg(unix)]
#[test]
fn test_a_workspace_watched_through_a_symlink_reports_an_external_add() {
    let real = tempfile::tempdir().expect("tempdir");
    let links = tempfile::tempdir().expect("tempdir for the link");
    let linked_root = links.path().join("workspace");
    std::os::unix::fs::symlink(real.path(), &linked_root).expect("symlink the workspace");
    std::fs::write(
        real.path().join("pustman.json"),
        r#"{"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","name":"W","format_version":"1.0"}"#,
    )
    .expect("write pustman.json");
    std::fs::create_dir_all(real.path().join("collections/c")).expect("mkdir");

    let (_watcher, rx) = watch(&linked_root).expect("watch through the symlink");
    std::fs::write(
        linked_root.join("collections/c/new.request.json"),
        r#"{"type":"http","id":"01ARZ3NDEKTSV4RRFFQ69G5FB3","name":"New","method":"GET","url":"https://example.com","headers":[]}"#,
    )
    .expect("write a request through the symlink");

    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("an external add is reported");
    assert!(
        matches!(event, FsChange::ItemAdded { .. }),
        "expected ItemAdded, got {event:?}"
    );
}
