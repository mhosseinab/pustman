//! Re-registering a panel id at another position (S22c): the panel *moves* — the dock
//! it leaves keeps no stale entry for the id, and its selection stays on a panel it
//! still holds. Without the move, the old dock would render an entry for a panel that
//! now lives in the other dock.

use gpui::{Entity, EntityId, FocusHandle, Focusable, TestAppContext, VisualTestContext};
use ui::prelude::*;
use ui::test_support::init_test;

use crate::{DockPosition, Panel, Workspace, init};

/// A minimal panel: its own focus handle, and an id, title and position the test picks.
struct TestPanel {
    focus_handle: FocusHandle,
    panel_id: &'static str,
    title: &'static str,
    position: DockPosition,
}

impl Render for TestPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .child(Label::new(self.title))
    }
}

impl Focusable for TestPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for TestPanel {
    fn panel_id(&self) -> &'static str {
        self.panel_id
    }

    fn title(&self, _cx: &App) -> SharedString {
        self.title.into()
    }

    fn icon(&self) -> IconName {
        IconName::Folder
    }

    fn position(&self, _cx: &App) -> DockPosition {
        self.position
    }
}

/// A workspace in its own window, on the app's real init order and the real default
/// keymap — the fixture shape `workspace_tests.rs` uses, kept local so the two test
/// files stay independent.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    cx.add_window_view(Workspace::new)
}

/// One [`TestPanel`] entity at `position`.
fn test_panel(
    cx: &mut TestAppContext,
    panel_id: &'static str,
    title: &'static str,
    position: DockPosition,
) -> Entity<TestPanel> {
    cx.update(|cx| {
        cx.new(|cx| TestPanel {
            focus_handle: cx.focus_handle(),
            panel_id,
            title,
            position,
        })
    })
}

/// Register `panel` with the workspace the way a feature crate would.
fn register_panel<T: Panel>(
    workspace: &Entity<Workspace>,
    panel: Entity<T>,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(panel, window, cx);
        });
    });
}

/// The panel ids the workspace's dock at `position` holds, in registration order.
fn panel_ids(
    workspace: &Entity<Workspace>,
    position: DockPosition,
    cx: &VisualTestContext,
) -> Vec<&'static str> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let dock = match position {
                DockPosition::Left => workspace.left_dock(),
                DockPosition::Bottom => workspace.bottom_dock(),
            };
            dock.read(cx)
                .panels()
                .iter()
                .map(|panel| panel.panel_id(cx))
                .collect()
        })
    })
}

/// The active panel of the workspace's dock at `position`, as its entity id.
fn active_panel_id(
    workspace: &Entity<Workspace>,
    position: DockPosition,
    cx: &VisualTestContext,
) -> Option<EntityId> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let dock = match position {
                DockPosition::Left => workspace.left_dock(),
                DockPosition::Bottom => workspace.bottom_dock(),
            };
            dock.read(cx).panel().map(|panel| panel.entity_id())
        })
    })
}

#[gpui::test]
fn re_registering_a_panel_at_another_position_leaves_no_entry_in_the_old_dock(
    cx: &mut TestAppContext,
) {
    let (workspace, cx) = workspace_window(cx);
    let first = test_panel(cx, "tools", "Tools", DockPosition::Left);
    register_panel(&workspace, first, cx);
    let moved = test_panel(cx, "tools", "Tools", DockPosition::Bottom);
    register_panel(&workspace, moved.clone(), cx);

    assert_eq!(
        panel_ids(&workspace, DockPosition::Left, cx),
        Vec::<&str>::new(),
        "the dock the panel left keeps no stale entry for its id"
    );
    assert_eq!(
        panel_ids(&workspace, DockPosition::Bottom, cx),
        vec!["tools"],
        "the panel is registered under its id in the dock it moved to"
    );
    assert_eq!(
        active_panel_id(&workspace, DockPosition::Bottom, cx),
        Some(moved.entity_id()),
        "the moved panel is active in its new dock"
    );
}

#[gpui::test]
fn moving_the_active_panel_away_keeps_the_old_dock_on_a_panel_it_holds(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let one = test_panel(cx, "one", "One", DockPosition::Left);
    let two = test_panel(cx, "two", "Two", DockPosition::Left);
    register_panel(&workspace, one, cx);
    register_panel(&workspace, two.clone(), cx);
    let moved = test_panel(cx, "one", "One", DockPosition::Bottom);
    register_panel(&workspace, moved, cx);

    assert_eq!(
        panel_ids(&workspace, DockPosition::Left, cx),
        vec!["two"],
        "the old dock holds only the panel that stayed"
    );
    assert_eq!(
        active_panel_id(&workspace, DockPosition::Left, cx),
        Some(two.entity_id()),
        "the old dock's selection follows the moved panel out"
    );
}
