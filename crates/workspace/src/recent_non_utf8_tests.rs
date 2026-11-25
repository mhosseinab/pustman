//! A recent workspace whose folder name is not UTF-8 (#332, Linux): the recent list
//! carries the path losslessly — through the store, the global and the menu action — so
//! choosing it from the switcher reopens that very folder instead of a lossy look-alike
//! that does not exist.

#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::ItemId;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test};
use workspace_store::FORMAT_VERSION;

use crate::recent_workspaces::{
    OpenRecentWorkspace, decode_recent_path, encode_recent_path, recent_workspaces,
};
use crate::title_bar::SWITCHER_SELECTOR;
use crate::workspace_open::WorkspaceOpen;
use crate::{Workspace, init};

/// `parent/caf\xe9` — Latin-1 "café", which is not valid UTF-8.
fn non_utf8_folder(parent: &Path) -> PathBuf {
    parent.join(OsString::from_vec(b"caf\xe9".to_vec()))
}

fn fixture_workspace(root: &Path, title: &str) {
    std::fs::create_dir_all(root).expect("the fixture folder is created");
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": title,
        "format_version": FORMAT_VERSION,
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_vec_pretty(&document).expect("the fixture serializes"),
    )
    .expect("the fixture workspace file is written");
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[test]
fn test_a_non_utf8_path_round_trips_through_the_recent_encoding() {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let folder = non_utf8_folder(directory.path());
    assert!(folder.to_str().is_none(), "the fixture name is not UTF-8");
    assert_eq!(decode_recent_path(&encode_recent_path(&folder)), folder);
    assert_eq!(
        OpenRecentWorkspace::for_folder(&folder).folder(),
        folder,
        "the menu action carries the exact folder"
    );
}

#[test]
fn test_a_utf8_path_is_stored_as_itself_and_never_misread_as_encoded() {
    let plain = PathBuf::from("/workspaces/billing");
    assert_eq!(encode_recent_path(&plain), "/workspaces/billing");
    assert_eq!(decode_recent_path("/workspaces/billing"), plain);
    let tricky = PathBuf::from("base64:Zm9v");
    assert_eq!(
        encode_recent_path(&tricky),
        "base64:Zm9v",
        "a UTF-8 path is stored as itself, whatever it starts with"
    );
    assert_eq!(
        decode_recent_path("base64:Zm9v"),
        tricky,
        "an old relative row that starts with \"base64:\" is read as the plain path it is"
    );
}

// Only Linux filesystems accept a non-UTF-8 name; APFS refuses it ("Illegal byte sequence").
#[gpui::test]
fn test_a_non_utf8_recent_workspace_reopens_from_the_switcher(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let cafe = non_utf8_folder(directory.path());
    let orders = directory.path().join("orders");
    fixture_workspace(&cafe, "Cafe API");
    fixture_workspace(&orders, "Orders API");

    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    cx.update(|_window, cx| WorkspaceOpen::install_local_store(Arc::clone(&store), cx));
    cx.run_until_parked();
    let opener = cx
        .update(|_window, cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has an opener");
    // Open the non-UTF-8 workspace (recording it), then another one over it.
    for folder in [cafe.clone(), orders.clone()] {
        opener.update(cx, |opener, cx| opener.open_folder(folder, cx));
        cx.run_until_parked();
    }
    draw_window(cx);

    let listed: Vec<PathBuf> = cx.update(|_window, cx| {
        recent_workspaces(cx)
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    });
    assert!(
        listed.contains(&cafe),
        "the recent list holds the exact non-UTF-8 folder, got {listed:?}"
    );

    click(cx, SWITCHER_SELECTOR);
    click(cx, "Cafe API");

    let root = cx.update(|_window, cx| opener.read(cx).root().map(Path::to_path_buf));
    assert_eq!(
        root,
        Some(cafe),
        "choosing it from the switcher reopens that very folder"
    );
}
