//! The sidebar's Environments view (S48u3): with no workspace it offers to open one;
//! an open workspace without environments offers New environment…, which writes one
//! into the workspace; a listed environment shows the "Active" badge when it is the
//! active one, and a click makes another one active.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId};
use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use pustman_actions::workspace::OpenWorkspace;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test};
use workspace::{Workspace, WorkspaceOpen};

use super::{
    EMPTY_ID, EMPTY_NEW_ID, EnvironmentsPanel, NO_WORKSPACE_ID, NO_WORKSPACE_OPEN_ID, PANEL_ID,
    row_id,
};
use crate::selector::{SelectorInputs, install_inputs};

fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace window showing the Environments view, with `folder` opened in it when
/// one is given.
fn open_panel<'a>(
    folder: Option<&Path>,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.update(init_test);
    cx.update(workspace::init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    let panel = cx.update(|_window, cx| {
        let weak = workspace.downgrade();
        cx.new(|cx| EnvironmentsPanel::new(weak, cx))
    });
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(panel, window, cx);
            workspace
                .left_dock()
                .update(cx, |dock, cx| dock.activate_panel(PANEL_ID, window, cx));
        });
    });
    if let Some(folder) = folder {
        let folder = folder.to_path_buf();
        cx.update(|_window, cx| {
            let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
                .expect("workspace::init attaches an opener");
            opener.update(cx, |opener, cx| opener.open_folder(folder, cx));
        });
    }
    cx.run_until_parked();
    draw_window(cx);
    (workspace, cx)
}

fn workspace_folder(parent: &Path) -> PathBuf {
    let root = parent.join("commerce");
    std::fs::create_dir_all(&root).expect("the workspace folder is created");
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Commerce",
        "format_version": workspace_store::FORMAT_VERSION,
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_string_pretty(&document).expect("the fixture serializes"),
    )
    .expect("pustman.json is written");
    root
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} renders"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

fn environment(name: &str) -> Environment {
    Environment {
        id: ItemId::new(),
        name: name.to_owned(),
        variables: Vec::new(),
        extra: Default::default(),
    }
}

#[gpui::test]
fn test_without_a_workspace_the_environments_view_offers_to_open_one(cx: &mut TestAppContext) {
    let opened = Rc::new(Cell::new(0));
    let (_workspace, cx) = open_panel(None, cx);
    cx.update(|_window, cx| {
        let opened = opened.clone();
        cx.on_action(move |_: &OpenWorkspace, _cx| opened.set(opened.get() + 1));
    });
    assert!(
        cx.debug_bounds(NO_WORKSPACE_ID).is_some(),
        "the no-workspace line renders"
    );
    click(cx, NO_WORKSPACE_OPEN_ID);
    assert!(
        opened.get() >= 1,
        "Open workspace… dispatches OpenWorkspace"
    );
}

#[gpui::test]
fn test_new_environment_writes_an_environment_into_the_open_workspace(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let root = workspace_folder(directory.path());
    let (_workspace, cx) = open_panel(Some(&root), cx);
    assert!(
        cx.debug_bounds(EMPTY_ID).is_some(),
        "an empty workspace says it has none"
    );

    click(cx, EMPTY_NEW_ID);
    let written: Vec<String> = std::fs::read_dir(root.join("environments"))
        .expect("the environments folder exists")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        written,
        vec!["new-environment.environment.json".to_owned()],
        "New environment… writes one environment file into the workspace"
    );
}

#[gpui::test]
fn test_the_active_environment_is_badged_and_a_click_activates_another(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let root = workspace_folder(directory.path());
    let (_workspace, cx) = open_panel(Some(&root), cx);
    let (development, staging) = (environment("Development"), environment("Staging"));
    let (development_id, staging_id) = (development.id, staging.id);
    cx.update(|_window, cx| {
        install_inputs(
            cx,
            SelectorInputs {
                workspace_id: "test-workspace".to_owned(),
                environments: vec![development, staging],
                active: Some(development_id),
                store: open_store(),
            },
        );
    });
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(format!("{}-active", row_id(development_id)).leak())
            .is_some(),
        "the active environment carries the Active badge"
    );
    assert!(
        cx.debug_bounds(format!("{}-active", row_id(staging_id)).leak())
            .is_none(),
        "only the active one does"
    );

    click(cx, row_id(staging_id).leak());
    let active = cx.update(|_window, cx| cx.global::<SelectorInputs>().active);
    assert_eq!(
        active,
        Some(staging_id),
        "a click makes that environment active"
    );
}
