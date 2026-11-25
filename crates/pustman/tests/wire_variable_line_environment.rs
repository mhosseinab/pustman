//! The request editor's variable line names the active environment in the running app
//! (S48u4 fix 1): with "Development" picked, a resolved name reads "Source:
//! Development" and an undefined one "is not defined in Development." — the scope wire
//! hands every editor the environment's display name beside its scopes.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind};
use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::environment::SelectEnvironment;
use pustman_actions::workspace::NewRequest;
use request_editor::RequestEditor;
use request_editor::variable_line::EntryState;
use storage::LocalStore;
use ui::test_support::draw_window;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path();
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Variable line workspace",
        "format_version": "1.0",
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_string_pretty(&document).expect("the fixture serializes"),
    )
    .expect("the workspace file is written");
    let development = Environment {
        id: ItemId::new(),
        name: "Development".to_string(),
        variables: vec![Variable {
            key: "base_url".to_string(),
            value: Some("https://api.dev.example.test".to_string()),
            kind: VariableKind::Default,
            enabled: true,
            description: None,
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    };
    workspace_store::save_item(
        root,
        &Item {
            body: ItemBody::Environment(development),
            dir: PathBuf::from("environments"),
            path: None,
        },
    )
    .expect("the environment is written");
    directory
}

fn active_editor(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Entity<RequestEditor> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
        })
    })
    .expect("the active tab is a request editor")
}

fn state_of(editor: &Entity<RequestEditor>, cx: &VisualTestContext, name: &str) -> EntryState {
    cx.read(|cx| {
        editor
            .read(cx)
            .variable_line()
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.state.clone())
            .unwrap_or_else(|| panic!("{name} is on the variable line"))
    })
}

#[gpui::test]
fn wire_variable_line_names_the_active_environment(cx: &mut TestAppContext) {
    let folder = fixture();
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    let opener = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    let root = folder.path().to_path_buf();
    cx.update(|_, cx| opener.update(cx, |opener, cx| opener.open_folder(root, cx)));
    cx.run_until_parked();
    draw_window(cx);

    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    cx.simulate_input("{{base_url}}/orders?limit={{page_size}}");
    let editor = active_editor(&workspace, cx);

    cx.dispatch_action(SelectEnvironment);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input("Development");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);

    match state_of(&editor, cx, "base_url") {
        EntryState::Resolved { source, .. } => assert_eq!(source, "Source: Development"),
        other => panic!("base_url resolves from Development, got {other:?}"),
    }
    match state_of(&editor, cx, "page_size") {
        EntryState::Unresolved { message, .. } => {
            assert_eq!(message, "is not defined in Development.")
        }
        other => panic!("page_size is undefined, got {other:?}"),
    }
}
