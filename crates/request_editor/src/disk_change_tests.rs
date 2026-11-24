//! The "Changed on disk" bar end to end (REQ-COLLAB-01): a workspace opened through
//! [`WorkspaceOpen::open_folder`] — never a hand-installed snapshot and never a
//! hand-made change event — whose request file is rewritten behind the app's back is
//! what the tab reacts to: the bar on a dirty tab, and the three ways past it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use api_model::{HttpRequest, ItemId, Request};
use gpui::{Entity, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};
use workspace::{Item, Workspace, WorkspaceOpen};
use workspace_store::{Item as StoreItem, ItemBody};

use crate::RequestEditor;
use crate::disk_change::{
    CHANGED_ON_DISK_SELECTOR, COMPARE_DISK_SELECTOR, COMPARE_MINE_SELECTOR, COMPARE_SELECTOR,
    CompareRow, KEEP_MINE_SELECTOR, RELOAD_SELECTOR,
};
use crate::save::{RequestFile, install};

/// The request the tests open: a GET whose file has a stable name.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.name = String::from("Get pets");
    request.url = String::from("https://example.com/pets");
    request
}

/// Write `root`'s workspace the way the app opens it (PRD §6.1): `pustman.json` and the
/// request's file, returned at the placement a save finds it.
fn fixture_workspace(root: &Path, request: &HttpRequest) -> RequestFile {
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Changed on disk workspace",
        "format_version": "1.0",
    });
    let bytes = serde_json::to_string_pretty(&document).expect("the workspace file serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the workspace file is written");
    let item = StoreItem {
        body: ItemBody::Request(Request::Http(request.clone())),
        dir: PathBuf::from("collections"),
        path: None,
    };
    let written = workspace_store::save_item(root, &item).expect("the fixture request saves");
    RequestFile {
        root: root.to_path_buf(),
        item: StoreItem {
            path: Some(written),
            ..item
        },
    }
}

/// Rewrite the request's file behind the app's back — the `git pull` case.
fn rewrite_file(root: &Path, file: &RequestFile, request: &HttpRequest) {
    let path = root.join(
        file.item
            .path
            .as_deref()
            .expect("the fixture request has a file"),
    );
    let bytes = serde_json::to_string_pretty(&Request::Http(request.clone()))
        .expect("the request serializes");
    std::fs::write(path, bytes).expect("the file is rewritten");
}

/// A workspace window holding `request` as its one tab, opened through the real open
/// path: [`WorkspaceOpen::open_folder`] loads the snapshot and this tab's watcher
/// starts over the folder exactly as in the running app.
fn open_tab(
    cx: &mut TestAppContext,
    request: HttpRequest,
    file: RequestFile,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    let root = file.root.clone();
    cx.update(init_test);
    cx.update(|cx| {
        workspace::init(cx);
        text_input::init(cx);
        crate::init(cx);
        install([file], cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    cx.update(|_, cx| {
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("every workspace window has its WorkspaceOpen");
        opener.update(cx, |opener, cx| opener.open_folder(root, cx));
    });
    cx.run_until_parked();
    let editor = cx.update(|window, cx| {
        let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        });
        editor
    });
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx)));
    cx.run_until_parked();
    (editor, cx)
}

/// The user's edit: type onto the URL, which turns the tab dirty.
fn edit_url(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();
}

/// Bounded wait for the changed-on-disk bar: the watcher is a real `notify` thread over
/// a real folder, so this passes real time but gives up after ~2 s instead of hanging —
/// a regression must fail, not stall.
async fn wait_for_bar(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> bool {
    for _ in 0..20 {
        if cx.read(|cx| editor.read_with(cx, |editor, _| editor.disk_change().is_some())) {
            return true;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    false
}

/// Click one control of the bar the way a user does.
fn click_control(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("the control is on screen");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The `workspace::Save` keystroke on this platform — Cmd on macOS, Ctrl elsewhere.
fn save_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-s"
    } else {
        "ctrl-s"
    }
}

#[gpui::test]
async fn dirty_tab_shows_changed_on_disk_bar(cx: &mut TestAppContext) {
    // The watcher under test is real IO over a real tempdir: parking lets real time
    // pass through the GPUI timers the waits below use (external.rs's seam).
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let (editor, cx) = open_tab(cx, request.clone(), file.clone());

    // The user edits the URL: the tab is dirty when the file changes.
    edit_url(cx);

    // The teammate's edit behind the app's back (REQ-COLLAB-01).
    let mut changed = request.clone();
    changed.url = String::from("https://example.com/pets?team=other");
    rewrite_file(temp.path(), &file, &changed);

    assert!(
        wait_for_bar(&editor, cx).await,
        "a dirty tab shows the changed-on-disk bar within the 1 s budget"
    );
    draw_window(cx);
    assert!(
        cx.debug_bounds(CHANGED_ON_DISK_SELECTOR).is_some(),
        "the bar is on screen"
    );

    // Compare is the third way past the bar: the side-by-side field diff.
    click_control(COMPARE_SELECTOR, cx);
    assert!(
        cx.debug_bounds(COMPARE_MINE_SELECTOR).is_some()
            && cx.debug_bounds(COMPARE_DISK_SELECTOR).is_some(),
        "Compare shows its two columns"
    );
    let rows = cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor
                .compare_rows()
                .map(|rows| rows.to_vec())
                .unwrap_or_default()
        })
    });
    assert!(
        rows.iter().any(
            |row| matches!(row, CompareRow::Line { mine: Some(mine), .. }
                if mine.ends_with("/v2"))
        ),
        "the Mine column shows the draft's URL: {rows:?}"
    );
    assert!(
        rows.iter().any(|row| matches!(
            row,
            CompareRow::Line { disk: Some(disk), .. }
                if disk == "https://example.com/pets?team=other"
        )),
        "the On disk column shows the file's URL: {rows:?}"
    );
}

#[gpui::test]
async fn reload_discards_draft(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let (editor, cx) = open_tab(cx, request.clone(), file.clone());

    edit_url(cx);
    let edited_url = cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.clone()));

    let mut changed = request.clone();
    changed.url = String::from("https://example.com/pets?team=other");
    rewrite_file(temp.path(), &file, &changed);

    assert!(
        wait_for_bar(&editor, cx).await,
        "the dirty tab shows the changed-on-disk bar"
    );
    draw_window(cx);
    click_control(RELOAD_SELECTOR, cx);

    assert_ne!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.clone())),
        edited_url,
        "Reload discards the typed edit"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url == changed.url)),
        "the tab holds the file's version"
    );
    assert!(
        !cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "a reloaded tab is clean again"
    );
    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.url_bar().read(cx).url(cx))),
        changed.url,
        "the rebuilt URL bar shows the file's URL"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.disk_change().is_none())),
        "the bar goes once the tab follows the file"
    );
}

#[gpui::test]
async fn keep_mine_overwrites_on_save(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (editor, cx) = open_tab(cx, request.clone(), file.clone());

    edit_url(cx);
    let edited_url = cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.clone()));

    let mut changed = request.clone();
    changed.url = String::from("https://example.com/pets?team=other");
    rewrite_file(temp.path(), &file, &changed);

    assert!(
        wait_for_bar(&editor, cx).await,
        "the dirty tab shows the changed-on-disk bar"
    );
    draw_window(cx);
    click_control(KEEP_MINE_SELECTOR, cx);

    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.disk_change().is_none())),
        "Keep mine dismisses the bar"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "Keep mine keeps the draft"
    );

    cx.simulate_keystrokes(save_keys(cx));
    cx.run_until_parked();

    let on_disk =
        std::fs::read_to_string(temp.path().join(&written)).expect("the request's file is there");
    let saved: Request = serde_json::from_str(&on_disk).expect("the file is the request's JSON");
    let Request::Http(saved) = saved else {
        panic!("the file holds an HTTP request");
    };
    assert_eq!(
        saved.url, edited_url,
        "the next save overwrites the file with the draft"
    );
    assert!(
        !cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "the save clears the dirty dot"
    );
}
