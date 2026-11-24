//! Quick open driven the way the app drives it: the request list comes from the
//! workspace `workspace::WorkspaceOpen` opens (the door `pustman::open_local_store`'s
//! app wiring feeds — no hand-installed inputs), `quick_open::Toggle`'s production
//! keybinding opens the picker, and Enter opens the chosen request's tab in the
//! workspace's center pane. A quick open that listed nothing, matched only names or
//! opened no tab fails here.

use std::collections::BTreeMap;
use std::path::Path;

use api_model::ItemId;
use gpui::{Entity, OperatingSystem, TestAppContext, VisualTestContext};
use request_editor::RequestEditor;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::{Workspace, WorkspaceOpen};

use crate::picker::Picker;

use super::QuickOpenDelegate;

/// The workspace folder at `root`: `pustman.json` and `collections/` as PRD §6.1 shapes
/// them (a collection directory per collection, one file per request), so
/// `workspace_store::open` reads a snapshot with three requests in it. Returns each
/// request's id keyed by the "Collection / Folder / Name" path quick open must list it
/// under.
fn fixture_workspace(root: &Path) -> BTreeMap<String, ItemId> {
    let billing = ItemId::new();
    let list_invoices = ItemId::new();
    let pet_store = ItemId::new();
    let create_user = ItemId::new();
    let users = ItemId::new();
    let get_user = ItemId::new();

    write_json(
        &root.join("pustman.json"),
        serde_json::json!({
            "id": ItemId::new().to_string(),
            "name": "Quick open workspace",
            "format_version": "1.0",
        }),
    );
    write_json(
        &root.join("collections/billing/collection.json"),
        serde_json::json!({
            "id": billing.to_string(),
            "name": "Billing",
            "order": [list_invoices.to_string()],
        }),
    );
    write_json(
        &root.join("collections/billing/list-invoices.request.json"),
        request_json(list_invoices, "List invoices", "GET"),
    );
    write_json(
        &root.join("collections/pet-store/collection.json"),
        serde_json::json!({
            "id": pet_store.to_string(),
            "name": "Pet Store",
            "order": [create_user.to_string(), users.to_string()],
        }),
    );
    write_json(
        &root.join("collections/pet-store/create-user.request.json"),
        request_json(create_user, "Create user", "POST"),
    );
    write_json(
        &root.join("collections/pet-store/users/folder.json"),
        serde_json::json!({
            "id": users.to_string(),
            "name": "Users",
            "order": [get_user.to_string()],
        }),
    );
    write_json(
        &root.join("collections/pet-store/users/get-user.request.json"),
        request_json(get_user, "Get user", "GET"),
    );

    BTreeMap::from([
        ("Billing / List invoices".to_string(), list_invoices),
        ("Pet Store / Create user".to_string(), create_user),
        ("Pet Store / Users / Get user".to_string(), get_user),
    ])
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
/// ([`WorkspaceOpen::open_folder`] — the folder dialog, the launch reopen and the
/// recent picker all call it), and wait for the snapshot to land. The snapshot itself
/// is read back through [`WorkspaceOpen`], never installed by hand.
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

/// The rows the open picker lists: each row's "Collection / Folder / Name" path and its
/// method badge text.
fn listed_rows(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    cx.update(|_, cx| {
        let picker = workspace
            .read_with(cx, |workspace, _| {
                workspace
                    .modal_layer()
                    .active_modal::<Picker<QuickOpenDelegate>>()
            })
            .expect("quick open is open");
        picker.read_with(cx, |picker, _| {
            picker
                .delegate()
                .listed()
                .into_iter()
                .map(|entry| (entry.display.clone(), entry.method.clone()))
                .collect()
        })
    })
}

/// The workspace's center pane: how many tabs it holds, and the request id behind its
/// active one.
fn tab_state(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> (usize, Option<ItemId>) {
    cx.update(|_, cx| {
        let pane = workspace.read(cx).center_pane().read(cx);
        let count = pane.item_ids().len();
        let active = pane.active_item().and_then(|item| {
            item.to_any_view()
                .downcast::<RequestEditor>()
                .ok()
                .map(|editor| editor.read(cx).draft().id)
        });
        (count, active)
    })
}

#[gpui::test]
fn quick_open_lists_all_requests_with_paths(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    fixture_workspace(directory.path());
    let (workspace, cx) = quick_open_window(cx);
    open_workspace_folder(&workspace, directory.path(), cx);

    press_key(cx, toggle_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        picker_is_open(&workspace, cx),
        "quick_open::Toggle opens quick open over the open workspace"
    );
    assert_eq!(
        listed_rows(&workspace, cx),
        vec![
            ("Billing / List invoices".to_string(), "GET".to_string()),
            ("Pet Store / Create user".to_string(), "POST".to_string()),
            (
                "Pet Store / Users / Get user".to_string(),
                "GET".to_string()
            ),
        ],
        "every request in the workspace is listed as Collection / Folder / Name with its method badge"
    );
}

#[gpui::test]
fn fuzzy_matches_on_path_segments(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    fixture_workspace(directory.path());
    let (workspace, cx) = quick_open_window(cx);
    open_workspace_folder(&workspace, directory.path(), cx);

    press_key(cx, toggle_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    // A query that spells the request out across two segments — its folder and its
    // name — matches it and nothing else.
    cx.simulate_input("users get");
    draw_window(cx);
    assert_eq!(
        listed_rows(&workspace, cx),
        vec![(
            "Pet Store / Users / Get user".to_string(),
            "GET".to_string()
        )],
        "a query matches across the path segments, not just the request's name"
    );

    // A fresh query over a different segment — the collection name.
    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);
    press_key(cx, toggle_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input("billing");
    draw_window(cx);
    assert_eq!(
        listed_rows(&workspace, cx),
        vec![("Billing / List invoices".to_string(), "GET".to_string())],
        "a query can land on the collection segment too"
    );
}

#[gpui::test]
fn enter_opens_request_tab(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let ids = fixture_workspace(directory.path());
    let get_user = ids["Pet Store / Users / Get user"];
    let (workspace, cx) = quick_open_window(cx);
    open_workspace_folder(&workspace, directory.path(), cx);

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
        (1, Some(get_user)),
        "Enter opens the chosen request's tab in the center pane"
    );
    assert!(
        !picker_is_open(&workspace, cx),
        "quick open closes once it opened the tab"
    );

    // Opening the same request again focuses the tab it already opened — one request
    // owns one tab (REQ-UI-02).
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
        (1, Some(get_user)),
        "re-opening focuses the existing tab instead of adding a second one"
    );
}
