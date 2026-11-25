//! The welcome pane's two states (S48u3, mockup section 2) driven the way a user drives
//! them: with no workspace open it offers New workspace… and Open workspace…, and each
//! dispatches its action; its Recent list opens the clicked folder in this window; with a
//! workspace open it offers New collection…, which runs the Collections panel's own
//! "＋ New" in the panel.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui::{Entity, FocusHandle, Focusable, Modifiers, TestAppContext, VisualTestContext, Window};
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use storage::{LocalStore, RecentWorkspace};
use time::{Duration, OffsetDateTime};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::welcome::{
    IMPORT_ID, NEW_COLLECTION_ID, NEW_WORKSPACE_ID, OPEN_WORKSPACE_ID, recent_row_id,
};
use crate::workspace_open::{WorkspaceOpen, create_workspace};
use crate::{DockPosition, Panel, Workspace, init};

gpui::actions!(welcome_test, [Probe]);

/// A Collections stand-in whose "＋ New" is [`Probe`], counted when its handler runs.
struct ProbePanel {
    focus_handle: FocusHandle,
    probes: Rc<Cell<usize>>,
}

impl Render for ProbePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|panel, _: &Probe, _window, _cx| {
                panel.probes.set(panel.probes.get() + 1);
            }))
            .child(Label::new("Collections"))
    }
}

impl Focusable for ProbePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ProbePanel {
    fn panel_id(&self) -> &'static str {
        "collections"
    }

    fn title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Collections")
    }

    fn icon(&self) -> IconName {
        IconName::Folder
    }

    fn position(&self, _cx: &App) -> DockPosition {
        DockPosition::Left
    }

    fn create_action(&self, _cx: &App) -> Option<Box<dyn gpui::Action>> {
        Some(Box::new(Probe))
    }
}

fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Record `folder` as opened `minutes_ago` minutes ago.
fn record_recent(store: &Arc<Mutex<LocalStore>>, folder: &Path, minutes_ago: i64) {
    let store = store.lock().expect("the store lock is free");
    store
        .recent_workspaces()
        .insert(&RecentWorkspace {
            path: folder.to_string_lossy().into_owned(),
            opened_at: OffsetDateTime::now_utc() - Duration::minutes(minutes_ago),
        })
        .expect("the recent row is written");
}

/// A real workspace folder named `name` under `parent`.
fn workspace_folder(parent: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    create_workspace(&root, name).expect("the fixture workspace is created");
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

fn open_root(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    cx.update(|_window, cx| {
        WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .and_then(|opener| opener.read(cx).root().map(Path::to_path_buf))
    })
}

#[gpui::test]
fn test_without_a_workspace_the_welcome_buttons_create_and_open_one(cx: &mut TestAppContext) {
    cx.update(init_test);
    let created = Rc::new(Cell::new(0));
    let opened = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let created = created.clone();
        cx.on_action(move |_: &CreateWorkspace, _cx| created.set(created.get() + 1));
        let opened = opened.clone();
        cx.on_action(move |_: &OpenWorkspace, _cx| opened.set(opened.get() + 1));
    });
    let (_workspace, cx) = cx.add_window_view(Workspace::new);
    draw_window(cx);

    assert!(
        cx.debug_bounds(IMPORT_ID).is_some(),
        "Import from Postman… is shown (disabled until a workspace exists)"
    );
    assert!(
        cx.debug_bounds(NEW_COLLECTION_ID).is_none(),
        "with no workspace there is nowhere to put a collection"
    );
    click(cx, NEW_WORKSPACE_ID);
    assert_eq!(
        created.get(),
        1,
        "New workspace… dispatches CreateWorkspace"
    );
    click(cx, OPEN_WORKSPACE_ID);
    assert_eq!(opened.get(), 1, "Open workspace… dispatches OpenWorkspace");
}

#[gpui::test]
fn test_clicking_a_recent_workspace_opens_it_in_this_window(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let older = workspace_folder(directory.path(), "billing");
    let newer = workspace_folder(directory.path(), "commerce");
    let store = open_store();
    record_recent(&store, &older, 10);
    record_recent(&store, &newer, 1);

    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        WorkspaceOpen::install_local_store(Arc::clone(&store), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        open_root(&workspace, cx),
        Some(newer.clone()),
        "the launch reopen brings back the newest workspace"
    );
    assert!(
        cx.debug_bounds(recent_row_id(&newer).leak()).is_none(),
        "the workspace already open is not offered again"
    );

    click(cx, recent_row_id(&older).leak());
    assert_eq!(
        open_root(&workspace, cx),
        Some(older),
        "clicking a Recent row opens that workspace in this window"
    );
}

#[gpui::test]
fn test_with_a_workspace_open_new_collection_runs_the_panels_create_action(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let root = workspace_folder(directory.path(), "commerce");
    let store = open_store();
    record_recent(&store, &root, 1);

    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        WorkspaceOpen::install_local_store(Arc::clone(&store), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let probes = Rc::new(Cell::new(0));
    let panel = cx.update(|_window, cx| {
        let probes = probes.clone();
        cx.new(|cx| ProbePanel {
            focus_handle: cx.focus_handle(),
            probes,
        })
    });
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(panel, window, cx)
        });
    });
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds(NEW_WORKSPACE_ID).is_none(),
        "with a workspace open the welcome stops asking for one"
    );
    click(cx, NEW_COLLECTION_ID);
    assert_eq!(
        probes.get(),
        1,
        "New collection… runs the Collections panel's own create action in the panel"
    );
}

#[gpui::test]
fn test_with_the_sidebar_hidden_new_collection_still_runs_in_the_panel(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let root = workspace_folder(directory.path(), "commerce");
    let store = open_store();
    record_recent(&store, &root, 1);

    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        WorkspaceOpen::install_local_store(Arc::clone(&store), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let probes = Rc::new(Cell::new(0));
    let panel = cx.update(|_window, cx| {
        let probes = probes.clone();
        cx.new(|cx| ProbePanel {
            focus_handle: cx.focus_handle(),
            probes,
        })
    });
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(panel.clone(), window, cx);
            workspace
                .left_dock()
                .update(cx, |dock, cx| dock.set_open(false, cx));
        });
    });
    cx.run_until_parked();
    draw_window(cx);

    click(cx, NEW_COLLECTION_ID);
    draw_window(cx);
    cx.run_until_parked();
    let (open, focused) = cx.update(|window, cx| {
        (
            workspace.read(cx).left_dock().read(cx).is_open(),
            panel.read(cx).focus_handle.is_focused(window),
        )
    });
    assert!(open, "the hidden sidebar comes back on the panel");
    assert!(focused, "with the keyboard in the panel");
    assert_eq!(
        probes.get(),
        1,
        "and the panel's create action runs even though the sidebar was hidden"
    );
}

#[gpui::test]
fn test_the_recent_list_follows_the_workspaces_opened_since_it_was_read(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let first = workspace_folder(directory.path(), "billing");
    let second = workspace_folder(directory.path(), "commerce");
    let store = open_store();
    record_recent(&store, &first, 1);

    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        WorkspaceOpen::install_local_store(Arc::clone(&store), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    let opener = cx
        .update(|_window, cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has an opener");
    for folder in [second.clone(), first.clone()] {
        opener.update(cx, |opener, cx| opener.open_folder(folder, cx));
        cx.run_until_parked();
    }
    draw_window(cx);
    assert_eq!(open_root(&workspace, cx), Some(first));
    assert!(
        cx.debug_bounds(recent_row_id(&second).leak()).is_some(),
        "a workspace opened after the list was first read is listed"
    );
}

#[gpui::test]
fn test_a_recent_click_with_nothing_to_open_it_says_so(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let folder = workspace_folder(directory.path(), "billing");
    let store = open_store();
    record_recent(&store, &folder, 1);
    cx.update(init_test);
    // No `workspace::init`: the window has no opener to open the folder with.
    cx.update(|cx| WorkspaceOpen::install_local_store(Arc::clone(&store), cx));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    draw_window(cx);
    let before = cx.update(|_window, cx| workspace.read(cx).toasts().read(cx).len());
    click(cx, recent_row_id(&folder).leak());
    let after = cx.update(|_window, cx| workspace.read(cx).toasts().read(cx).len());
    assert_eq!(
        after,
        before + 1,
        "the click is answered with a toast, not silence"
    );
}
