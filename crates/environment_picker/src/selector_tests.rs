//! The selector is driven the way a user drives it: `environment::SelectEnvironment`
//! through GPUI's action dispatch, the picker's own keys, and the real
//! `storage::LocalStore` — with every assertion read back through a *fresh* store handle
//! on the same database, the way a reopened app would find it. A selection that is only
//! remembered in memory fails these tests.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId};
use command_palette::Picker;
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};
use pustman_actions::environment::SelectEnvironment;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Workspace;

use super::{EnvironmentSelector, SelectorInputs, install_inputs, selection_key};

/// Two environments, in the order the selector lists them.
fn environments() -> (Environment, Environment) {
    (environment("Staging"), environment("Production"))
}

/// One environment with no variables: the selector lists names and stores ids.
fn environment(name: &str) -> Environment {
    Environment {
        id: ItemId::new(),
        name: name.to_string(),
        variables: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// The local store the selection is persisted into: one real file-backed database, so a
/// fresh handle can read back what was written.
fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The kv rows of a freshly opened handle on the same database, keyed by row key.
fn read_selections(path: &Path) -> BTreeMap<String, String> {
    let fresh = LocalStore::open(path).expect("the store reopens");
    fresh
        .kv()
        .list()
        .expect("the kv rows read")
        .into_iter()
        .map(|row| (row.key, row.value))
        .collect()
}

/// The app's real init order plus this crate over the bundled defaults — and
/// `text_input::init`, because the picker's query field is a real [`text_input::TextInput`]
/// and its editing keys are unbound without it.
fn selector_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        command_palette::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// One pick round: install `inputs`, dispatch `environment::SelectEnvironment`, drive the
/// picker with `keys` the way a user does, and let the background write land.
fn pick(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
    inputs: SelectorInputs,
    keys: &[&str],
) {
    cx.update(|_window, cx| install_inputs(cx, inputs));
    cx.dispatch_action(SelectEnvironment);
    cx.run_until_parked();
    draw_window(cx);
    let picker = picker_of(workspace, cx).expect("dispatching SelectEnvironment opens the picker");
    let focus_handle = cx.read(|cx| picker.read_with(cx, |picker, cx| picker.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    for key in keys {
        press_key(cx, key);
    }
    cx.run_until_parked();
}

/// The picker the workspace's modal layer is showing, if the environment picker is open.
fn picker_of(
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

/// The inputs one pick round runs with.
fn inputs(
    workspace_id: &str,
    environments: &[Environment],
    active: Option<ItemId>,
    store: Arc<Mutex<LocalStore>>,
) -> SelectorInputs {
    SelectorInputs {
        workspace_id: workspace_id.to_string(),
        environments: environments.to_vec(),
        active,
        store,
    }
}

#[gpui::test]
fn selecting_environment_persists_per_workspace(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("pustman.db");
    let store = open_store(&path);
    let (staging, production) = environments();
    let both = [staging.clone(), production.clone()];
    let (workspace, cx) = selector_window(cx);

    // "down" moves off the "No environment" row onto Staging; Enter confirms.
    pick(
        cx,
        &workspace,
        inputs("workspace-a", &both, None, Arc::clone(&store)),
        &["down", "enter"],
    );
    // Two moves: past "No environment" and Staging onto Production.
    pick(
        cx,
        &workspace,
        inputs("workspace-b", &both, None, store),
        &["down", "down", "enter"],
    );

    let selections = read_selections(&path);
    assert_eq!(
        selections.len(),
        2,
        "the two workspaces keep two separate keys"
    );
    assert_eq!(
        selections.get(&selection_key("workspace-a")),
        Some(&staging.id.to_string()),
        "workspace-a stores the environment it picked"
    );
    assert_eq!(
        selections.get(&selection_key("workspace-b")),
        Some(&production.id.to_string()),
        "workspace-b keeps its own choice, untouched by workspace-a's"
    );
}

#[gpui::test]
fn no_environment_option_clears_selection(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("pustman.db");
    let store = open_store(&path);
    let (staging, production) = environments();
    let both = [staging.clone(), production.clone()];
    let (workspace, cx) = selector_window(cx);

    pick(
        cx,
        &workspace,
        inputs("workspace-clear", &both, None, Arc::clone(&store)),
        &["down", "enter"],
    );
    assert_eq!(
        read_selections(&path).get(&selection_key("workspace-clear")),
        Some(&staging.id.to_string()),
        "the first round stores the picked environment"
    );

    // Reopened on the stored choice (preselected), the first-class "No environment" row
    // is one arrow away — "up" wraps onto it — and Enter clears the selection.
    pick(
        cx,
        &workspace,
        inputs("workspace-clear", &both, Some(staging.id), store),
        &["up", "enter"],
    );
    assert!(
        !read_selections(&path).contains_key(&selection_key("workspace-clear")),
        "choosing \"No environment\" clears the stored selection"
    );
}
