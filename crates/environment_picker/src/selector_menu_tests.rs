//! The environment menu (S48u6, mockup section 6): a search field with "＋" at its
//! right, "No environment" and the environments with the active one checked, type to
//! filter, Enter to pick, and "Edit environments…" under the list.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId};
use command_palette::{Picker, PickerDelegate};
use gpui::{Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use pustman_actions::environment::SelectEnvironment;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::{Workspace, WorkspaceOpen};

use super::{
    EDIT_ENVIRONMENTS_BUTTON, EnvironmentSelector, NEW_ENVIRONMENT_BUTTON, SelectorInputs,
    install_inputs, selection_key,
};

fn environment(name: &str) -> Environment {
    Environment {
        id: ItemId::new(),
        name: name.to_string(),
        variables: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

fn workspace_folder(parent: &Path) -> PathBuf {
    let root = parent.join("orbit");
    std::fs::create_dir_all(&root).expect("the workspace folder is created");
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Orbit",
        "format_version": workspace_store::FORMAT_VERSION,
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_string_pretty(&document).expect("the fixture serializes"),
    )
    .expect("pustman.json is written");
    root
}

/// A workspace window with `folder` open (when given) and the menu open over `inputs`,
/// its search field focused.
fn open_menu<'a>(
    folder: Option<&Path>,
    inputs: SelectorInputs,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        command_palette::init(cx);
        workspace::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    if let Some(folder) = folder {
        let folder = folder.to_path_buf();
        cx.update(|_window, cx| {
            let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
                .expect("workspace::init attaches an opener");
            opener.update(cx, |opener, cx| opener.open_folder(folder, cx));
        });
    }
    cx.run_until_parked();
    cx.update(|_window, cx| install_inputs(cx, inputs));
    cx.dispatch_action(SelectEnvironment);
    cx.run_until_parked();
    draw_window(cx);
    let picker = menu_of(&workspace, cx).expect("SelectEnvironment opens the menu");
    let focus_handle = cx.read(|cx| picker.read_with(cx, |picker, cx| picker.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (workspace, cx)
}

fn menu_of(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Option<Entity<Picker<EnvironmentSelector>>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<Picker<EnvironmentSelector>>()
        })
    })
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} renders"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn test_typing_filters_the_menu_and_enter_picks_the_match(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("pustman.db");
    let store = open_store(&path);
    let (development, staging, production) = (
        environment("Development"),
        environment("Staging"),
        environment("Production"),
    );
    let inputs = SelectorInputs {
        workspace_id: "workspace-menu".to_string(),
        environments: vec![development.clone(), staging, production.clone()],
        active: Some(development.id),
        store,
    };
    let (workspace, cx) = open_menu(None, inputs, cx);

    cx.simulate_input("prod");
    draw_window(cx);
    let picker = menu_of(&workspace, cx).expect("the menu stays open while typing");
    let count = cx.read(|cx| picker.read(cx).delegate().match_count());
    assert_eq!(count, 1, "\"prod\" leaves Production alone in the list");

    press_key(cx, "enter");
    cx.run_until_parked();

    assert!(menu_of(&workspace, cx).is_none(), "picking closes the menu");
    let fresh = LocalStore::open(&path).expect("the store reopens");
    let stored = fresh
        .kv()
        .list()
        .expect("the kv rows read")
        .into_iter()
        .find(|row| row.key == selection_key("workspace-menu"))
        .map(|row| row.value);
    assert_eq!(
        stored,
        Some(production.id.to_string()),
        "the filtered match is the active environment now"
    );
    cx.read(|cx| {
        assert_eq!(
            cx.global::<SelectorInputs>().active,
            Some(production.id),
            "the status bar's readout follows the pick"
        );
    });
}

#[gpui::test]
fn test_the_plus_creates_a_new_environment_in_the_open_workspace(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = workspace_folder(directory.path());
    let store = open_store(&directory.path().join("pustman.db"));
    let inputs = SelectorInputs {
        workspace_id: "workspace-new".to_string(),
        environments: vec![environment("Development")],
        active: None,
        store,
    };
    let (workspace, cx) = open_menu(Some(&root), inputs, cx);

    click(cx, NEW_ENVIRONMENT_BUTTON);

    assert!(menu_of(&workspace, cx).is_none(), "the menu closes on ＋");
    let written: Vec<String> = std::fs::read_dir(root.join("environments"))
        .expect("the environments folder exists")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        written,
        vec!["new-environment.environment.json".to_owned()],
        "＋ writes one new environment file, the sidebar's create path"
    );
}

#[gpui::test]
fn test_edit_environments_closes_the_menu_for_the_editor(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let store = open_store(&directory.path().join("pustman.db"));
    let inputs = SelectorInputs {
        workspace_id: "workspace-edit".to_string(),
        environments: vec![environment("Development")],
        active: None,
        store,
    };
    let (workspace, cx) = open_menu(None, inputs, cx);
    assert!(
        cx.debug_bounds(EDIT_ENVIRONMENTS_BUTTON).is_some(),
        "the footer offers Edit environments…"
    );

    click(cx, EDIT_ENVIRONMENTS_BUTTON);

    assert!(
        menu_of(&workspace, cx).is_none(),
        "the menu makes way for the variables editor"
    );
}
