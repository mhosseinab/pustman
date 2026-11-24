//! A workspace whose `pustman.json` declares a newer major `format_version` opens
//! read-only (REQ-WSP-04): the banner is on screen, the flag items query reports
//! read-only, and a save press does nothing except show its toast — the folder is left
//! exactly as it was found. Driven the way a user drives it: the `OpenWorkspace` action
//! and its folder dialog, and the `Save` / `SaveAll` presses.

use std::path::Path;

use api_model::ItemId;
use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman_actions::workspace::{OpenWorkspace, Save, SaveAll};
use ui::components::banner::banner_message_selector;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};

use super::{READ_ONLY_MESSAGE, SAVE_REFUSED_MESSAGE, is_read_only};
use crate::{Workspace, init};

/// A workspace window on the app's real init order (the `workspace_open_tests.rs` shape,
/// minus the local store: nothing under test records into it).
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    (workspace, cx)
}

/// The workspace folder the open reads: a `pustman.json` declaring a newer major
/// `format_version` than this build writes (REQ-WSP-04).
fn newer_major_workspace(root: &Path) {
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Pets workspace",
        "format_version": "2.0",
    });
    let bytes = serde_json::to_string_pretty(&document).expect("the fixture serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the fixture file is written");
}

/// A `debug_bounds` key is `'static`, so the runtime-built selector is leaked once and
/// asserted on by value (the `toast_layer_tests.rs` shape).
fn leaked(selector: String) -> &'static str {
    selector.leak()
}

#[gpui::test]
fn newer_major_opens_read_only_with_banner(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path();
    newer_major_workspace(root);
    let (_workspace, cx) = workspace_window(cx);

    draw_window(cx);
    let banner = leaked(banner_message_selector(READ_ONLY_MESSAGE));
    assert!(
        cx.debug_bounds(banner).is_none(),
        "no read-only banner before any workspace is open"
    );

    // The user opens the folder the newer workspace lives in.
    cx.dispatch_action(OpenWorkspace);
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_paths(),
        "OpenWorkspace asks for the workspace folder in a dialog"
    );
    cx.simulate_path_prompt_response(|_| Some(vec![root.to_path_buf()]));
    cx.run_until_parked();

    // The flag items query reports read-only, so items disable their editing (REQ-WSP-04).
    assert!(
        cx.read(is_read_only),
        "a newer major format_version opens the workspace read-only"
    );

    // The banner states the fact where the user looks for it.
    draw_window(cx);
    assert!(
        cx.debug_bounds(banner).is_some(),
        "the read-only banner renders in the shell"
    );

    // A save press does nothing except say so.
    let untouched = std::fs::read(root.join("pustman.json")).expect("the workspace file reads");
    let toast = leaked(toast_message_selector(SAVE_REFUSED_MESSAGE));
    cx.dispatch_action(Save);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(toast).is_some(),
        "Save shows the read-only toast instead of writing"
    );
    cx.dispatch_action(SaveAll);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(toast).is_some(),
        "SaveAll is refused the same way"
    );
    assert_eq!(
        std::fs::read(root.join("pustman.json")).expect("the workspace file reads"),
        untouched,
        "save actions leave the newer workspace's files as they found them"
    );
}
