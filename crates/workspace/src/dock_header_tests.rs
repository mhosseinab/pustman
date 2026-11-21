//! The dock's panel header row (REQ-UI-01/04): a full-width row with the active panel's
//! title in the default text colour at the left and the dock's actions at the right —
//! the workbench blueprint's sidebar and Console bar headers — instead of a small
//! tinted selector chip floating in the dock's top-left corner.

use gpui::{
    Bounds, Entity, FocusHandle, Focusable, Modifiers, Pixels, TestAppContext, VisualTestContext,
    Window,
};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::dock::{dock_header_selector, dock_hide_selector, dock_panel_selector};
use crate::{DockPosition, Panel, Workspace, init};

/// A minimal panel: its own focus handle, and an id, title and position the test picks.
struct TestPanel {
    focus_handle: FocusHandle,
    panel_id: &'static str,
    title: &'static str,
    position: DockPosition,
}

impl TestPanel {
    fn new(
        panel_id: &'static str,
        title: &'static str,
        position: DockPosition,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            panel_id,
            title,
            position,
        }
    }
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
        IconName::History
    }

    fn position(&self, _cx: &App) -> DockPosition {
        self.position
    }
}

/// A workspace in its own window, on the app's real init order.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    cx.add_window_view(Workspace::new)
}

/// One [`TestPanel`] entity.
fn test_panel(
    cx: &mut TestAppContext,
    panel_id: &'static str,
    title: &'static str,
    position: DockPosition,
) -> Entity<TestPanel> {
    cx.update(|cx| cx.new(|cx| TestPanel::new(panel_id, title, position, cx)))
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

/// The rendered bounds of one header entry, under `dock_panel_selector`.
/// `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector string is
/// leaked — fine for a test's short life.
fn entry_bounds(cx: &mut VisualTestContext, panel_id: &'static str) -> Option<Bounds<Pixels>> {
    let selector: &'static str = dock_panel_selector(panel_id).leak();
    cx.debug_bounds(selector)
}

/// Whether `outer` fully contains `inner`.
fn contains_bounds(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.origin.x + inner.size.width <= outer.origin.x + outer.size.width
        && inner.origin.y + inner.size.height <= outer.origin.y + outer.size.height
}

/// The rendered bounds of the dock's header row or its hide action.
fn header_bounds(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    let selector: &'static str = selector.leak();
    cx.debug_bounds(selector)
}

#[gpui::test]
fn panel_header_row_spans_the_full_width_of_its_dock(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let history = test_panel(cx, "history", "History", DockPosition::Left);
    register_panel(&workspace, history, cx);
    draw_window(cx);

    let dock = cx.debug_bounds("left-dock").expect("the left dock renders");
    let header = header_bounds(cx, dock_header_selector(DockPosition::Left))
        .expect("the header row renders");
    assert!(
        contains_bounds(dock, header),
        "the panel header row renders inside its dock"
    );
    assert!(
        (dock.size.width - header.size.width).abs() <= px(2.),
        "the panel header row spans the dock's full width — it is not a chip floating in \
         the corner ({dock:?} vs {header:?})"
    );
}

#[gpui::test]
fn panel_header_puts_the_title_left_and_the_actions_right(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let collections = test_panel(cx, "collections", "Collections", DockPosition::Left);
    let history = test_panel(cx, "history", "History", DockPosition::Left);
    register_panel(&workspace, collections, cx);
    register_panel(&workspace, history, cx);
    draw_window(cx);

    let dock = cx.debug_bounds("left-dock").expect("the left dock renders");
    let title = entry_bounds(cx, "collections").expect("the active panel's title renders");
    let switch = entry_bounds(cx, "history").expect("the other panel's entry renders");
    let hide = header_bounds(cx, dock_hide_selector(DockPosition::Left))
        .expect("the header's hide action renders");

    assert!(
        title.size.height >= px(24.),
        "the panel title is a readable header row, not a tiny chip ({title:?})"
    );
    assert!(
        title.origin.x - dock.origin.x <= px(28.),
        "the title sits at the dock's left edge ({title:?} in {dock:?})"
    );
    assert!(
        dock.origin.x + dock.size.width - (switch.origin.x + switch.size.width) <= px(28.),
        "the panel actions sit at the right end of the header row ({switch:?} in {dock:?})"
    );
    assert!(
        switch.origin.x >= title.origin.x + title.size.width,
        "the title and the actions do not overlap ({title:?} vs {switch:?})"
    );
    assert!(
        hide.origin.x >= switch.origin.x + switch.size.width - px(1.),
        "the hide action is the header's rightmost entry ({hide:?} vs {switch:?})"
    );
}

#[gpui::test]
fn clicking_the_header_hide_action_hides_the_dock(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let history = test_panel(cx, "history", "History", DockPosition::Left);
    register_panel(&workspace, history, cx);
    draw_window(cx);

    let hide = header_bounds(cx, dock_hide_selector(DockPosition::Left))
        .expect("the header's hide action renders");
    cx.simulate_click(hide.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    let left_open = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| workspace.left_dock().read(cx).is_open())
    });
    assert!(
        !left_open,
        "the header's hide action closes its dock, the way ToggleSidebar does"
    );
    assert!(
        cx.debug_bounds("left-dock").is_none(),
        "the closed dock renders nothing"
    );
}
