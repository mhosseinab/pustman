//! Quick open's confirm path against tabs it did not open itself (S46 fix 1): a request
//! already open from the collections tree gets its tab reused — never a second editor
//! for one request — and the request behind the tab is the one the workspace holds
//! *now*, not the clone the row was listed from (fix 3). Everything is driven through
//! the app's own doors: [`WorkspaceOpen::open_folder`] fills the snapshot and plain
//! [`RequestEditor`] tabs land in the pane the way the tree adds them.

use std::collections::BTreeMap;
use std::path::Path;

use api_model::{HttpRequest, ItemId, Request};
use gpui::{
    AppContext, Entity, EntityId, Focusable, OperatingSystem, TestAppContext, VisualTestContext,
};
use request_editor::RequestEditor;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::{ItemHandle, Workspace, WorkspaceOpen};
use workspace_store::ItemBody;

use crate::picker::Picker;
use crate::quick_open::{QuickOpenDelegate, gone_message};

/// The workspace folder at `root`: `pustman.json` and `collections/` as PRD §6.1 shapes
/// them, with `Pet Store` holding `Create user` and `Get user`. Returns each request's id
/// keyed by the "Collection / Name" path quick open lists it under.
fn fixture_workspace(root: &Path) -> BTreeMap<String, ItemId> {
    let pet_store = ItemId::new();
    let create_user = ItemId::new();
    let get_user = ItemId::new();

    write_json(
        &root.join("pustman.json"),
        serde_json::json!({
            "id": ItemId::new().to_string(),
            "name": "Tab reuse workspace",
            "format_version": "1.0",
        }),
    );
    write_json(
        &root.join("collections/pet-store/collection.json"),
        serde_json::json!({
            "id": pet_store.to_string(),
            "name": "Pet Store",
            "order": [create_user.to_string(), get_user.to_string()],
        }),
    );
    write_json(
        &root.join("collections/pet-store/create-user.request.json"),
        request_json(create_user, "Create user", "POST"),
    );
    write_json(
        &root.join("collections/pet-store/get-user.request.json"),
        request_json(get_user, "Get user", "GET"),
    );

    BTreeMap::from([
        ("Pet Store / Create user".to_string(), create_user),
        ("Pet Store / Get user".to_string(), get_user),
    ])
}

/// A second workspace folder at `root`, holding one unrelated request — the workspace a
/// quick open listed before this folder lands must not open anything out of.
fn other_fixture_workspace(root: &Path) {
    let invoices = ItemId::new();
    write_json(
        &root.join("pustman.json"),
        serde_json::json!({
            "id": ItemId::new().to_string(),
            "name": "Other workspace",
            "format_version": "1.0",
        }),
    );
    write_json(
        &root.join("collections/invoices/collection.json"),
        serde_json::json!({
            "id": invoices.to_string(),
            "name": "Invoices",
            "order": [],
        }),
    );
}

/// One request file's JSON (PRD §6.1): the tagged shape `workspace_store` reads back.
fn request_json(id: ItemId, name: &str, method: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "http",
        "id": id.to_string(),
        "name": name,
        "method": method,
        "url": "https://api.example.test/",
    })
}

fn write_json(path: &Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the fixture directories are created");
    }
    let bytes = serde_json::to_string_pretty(&value).expect("the fixture serializes");
    std::fs::write(path, bytes).expect("the fixture is written");
}

/// One window on the app's real init order and the real default keymap, so quick open
/// opens on the production `quick_open::Toggle` binding.
fn quick_open_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        crate::init(cx);
        workspace::init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// Open `root` as this window's workspace through the one door every open lands in
/// ([`WorkspaceOpen::open_folder`]), and wait for the snapshot to land.
fn open_workspace_folder(workspace: &Entity<Workspace>, root: &Path, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("every workspace window has its WorkspaceOpen");
        opener.update(cx, |opener, cx| {
            opener.open_folder(root.to_path_buf(), cx);
        });
    });
    cx.run_until_parked();
    draw_window(cx);
}

/// The production `quick_open::Toggle` keystroke on this platform — Cmd on macOS, Ctrl
/// elsewhere, exactly as `assets/keymaps/` binds it.
fn toggle_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-p"
    } else {
        "ctrl-p"
    }
}

/// Whether quick open is the active modal.
fn picker_is_open(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<Picker<QuickOpenDelegate>>()
                .is_some()
        })
    })
}

/// Open `id`'s request in a plain [`RequestEditor`] tab, the way
/// `collections_panel`'s tree does: the request straight out of the open workspace's
/// snapshot, `add_tab_item`, focus in the URL bar. Nothing tells quick open about it.
fn open_tab_the_tree_does(workspace: &Entity<Workspace>, id: ItemId, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let request = current_request(workspace, cx, id).expect("the fixture request is openable");
        let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
        let pane = workspace.read(cx).center_pane().clone();
        pane.update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    cx.run_until_parked();
    draw_window(cx);
}

/// The request `id` names in the workspace's snapshot right now, as the tree opens it.
fn current_request(
    workspace: &Entity<Workspace>,
    cx: &gpui::App,
    id: ItemId,
) -> Option<HttpRequest> {
    let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)?;
    let body = opener.read(cx).snapshot()?.items.get(&id)?.body.clone();
    match body {
        ItemBody::Request(Request::Http(http)) => Some(http),
        _ => None,
    }
}

/// The workspace's center pane: how many tabs it holds, and the request id and entity id
/// behind its active one.
fn tab_state(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> (usize, Option<ItemId>, Option<EntityId>) {
    cx.update(|_, cx| {
        let pane = workspace.read(cx).center_pane().read(cx);
        let active = pane.active_item();
        (
            pane.item_ids().len(),
            active
                .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
                .map(|editor| editor.read(cx).draft().id),
            active.map(ItemHandle::entity_id),
        )
    })
}

/// Whether the active tab's request editor holds keyboard focus — what "opening the tab
/// again focuses it" means for the keyboard (the S39b tree test's bar).
fn active_editor_is_focused(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let editor = workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
        });
        editor.is_some_and(|editor| {
            let focus_handle = editor.read(cx).focus_handle(cx);
            focus_handle.contains_focused(window, cx)
        })
    })
}

#[gpui::test]
fn quick_open_reuses_a_tab_another_opener_opened(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let ids = fixture_workspace(directory.path());
    let get_user = ids["Pet Store / Get user"];
    let (workspace, cx) = quick_open_window(cx);
    open_workspace_folder(&workspace, directory.path(), cx);

    // Two tabs from another opener (the tree), the chosen request in a background tab.
    open_tab_the_tree_does(&workspace, get_user, cx);
    let (_, _, added) = tab_state(&workspace, cx);
    let get_user_tab = added.expect("the tree's tab is active as it lands");
    open_tab_the_tree_does(&workspace, ids["Pet Store / Create user"], cx);

    press_key(cx, toggle_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input("get user");
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        tab_state(&workspace, cx),
        (2, Some(get_user), Some(get_user_tab)),
        "quick open activates the editor the tree opened instead of adding a second one"
    );
    assert!(
        active_editor_is_focused(&workspace, cx),
        "the reused tab takes keyboard focus"
    );
}

#[gpui::test]
fn confirm_opens_the_request_the_workspace_holds_now(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    fixture_workspace(directory.path());
    let (workspace, cx) = quick_open_window(cx);
    open_workspace_folder(&workspace, directory.path(), cx);

    press_key(cx, toggle_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        picker_is_open(&workspace, cx),
        "quick open lists the workspace it was opened over"
    );

    // The workspace changes under the open picker — the request the row names is gone
    // from the snapshot quick open must confirm against.
    let other = tempfile::tempdir().expect("the temp dir must be created");
    other_fixture_workspace(other.path());
    open_workspace_folder(&workspace, other.path(), cx);

    // The confirmed row is the one the query names — the request the workspace held
    // when quick open listed it and no longer holds.
    cx.simulate_input("get user");
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        tab_state(&workspace, cx),
        (0, None, None),
        "a request the workspace no longer holds opens no tab from a stale row"
    );
    assert!(
        !picker_is_open(&workspace, cx),
        "quick open closes once it answered the confirm"
    );
    assert!(
        cx.debug_bounds(toast_message_selector(&gone_message("Pet Store / Get user")).leak())
            .is_some(),
        "the user is told the chosen request is gone"
    );
}
