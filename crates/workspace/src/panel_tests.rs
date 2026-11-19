//! Dock panel registration (S22c): a feature crate registers a [`Panel`] and its dock
//! grows a selector entry for it — one entry per panel, the first one active, and a
//! click on another entry switching to it and moving keyboard focus into it. Every
//! assertion is rendered-side (`debug_bounds` under the panel id's own hitbox) or a
//! registration outcome (the dock's active panel), so a dock that ignores what it is
//! given cannot pass.

use gpui::{
    Bounds, Entity, EntityId, FocusHandle, Focusable, Modifiers, Pixels, TestAppContext,
    VisualTestContext,
};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::dock::dock_panel_selector;
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

/// The rendered bounds of one selector entry, under `dock_panel_selector`.
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

#[gpui::test]
fn panel_registration_adds_dock_entry(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let collections = test_panel(cx, "collections", "Collections", DockPosition::Left);
    let console = test_panel(cx, "console", "Console", DockPosition::Bottom);
    register_panel(&workspace, collections.clone(), cx);
    register_panel(&workspace, console.clone(), cx);
    draw_window(cx);

    // Rendered-side proof: each entry renders inside the dock its panel registered for.
    let left_dock = cx.debug_bounds("left-dock").expect("the left dock renders");
    let bottom_dock = cx
        .debug_bounds("bottom-dock")
        .expect("the bottom dock renders");
    let collections_entry = entry_bounds(cx, "collections").expect("the collections entry renders");
    let console_entry = entry_bounds(cx, "console").expect("the console entry renders");
    assert!(
        contains_bounds(left_dock, collections_entry),
        "the collections entry renders inside the left dock"
    );
    assert!(
        contains_bounds(bottom_dock, console_entry),
        "the console entry renders inside the bottom dock"
    );

    assert_eq!(
        active_panel_id(&workspace, DockPosition::Left, cx),
        Some(collections.entity_id()),
        "the left dock shows the panel registered for it"
    );
    assert_eq!(
        active_panel_id(&workspace, DockPosition::Bottom, cx),
        Some(console.entity_id()),
        "the bottom dock shows the panel registered for it"
    );
}

#[gpui::test]
fn registering_a_second_panel_adds_a_second_entry_and_keeps_the_first_active(
    cx: &mut TestAppContext,
) {
    let (workspace, cx) = workspace_window(cx);
    let collections = test_panel(cx, "collections", "Collections", DockPosition::Left);
    let history = test_panel(cx, "history", "History", DockPosition::Left);
    register_panel(&workspace, collections.clone(), cx);
    register_panel(&workspace, history.clone(), cx);
    draw_window(cx);

    let left_dock = cx.debug_bounds("left-dock").expect("the left dock renders");
    let collections_entry = entry_bounds(cx, "collections").expect("the first entry still renders");
    let history_entry = entry_bounds(cx, "history").expect("the second entry renders");
    assert!(
        contains_bounds(left_dock, collections_entry),
        "the first entry keeps rendering inside the left dock"
    );
    assert!(
        contains_bounds(left_dock, history_entry),
        "the second entry renders inside the left dock too"
    );

    assert_eq!(
        active_panel_id(&workspace, DockPosition::Left, cx),
        Some(collections.entity_id()),
        "registering a second panel keeps the first one active"
    );
}

#[gpui::test]
fn clicking_a_dock_entry_activates_and_focuses_its_panel(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let collections = test_panel(cx, "collections", "Collections", DockPosition::Left);
    let history = test_panel(cx, "history", "History", DockPosition::Left);
    register_panel(&workspace, collections.clone(), cx);
    register_panel(&workspace, history.clone(), cx);
    draw_window(cx);

    let history_entry = entry_bounds(cx, "history").expect("the history entry renders");
    cx.simulate_click(history_entry.center(), Modifiers::none());
    draw_window(cx);

    assert_eq!(
        active_panel_id(&workspace, DockPosition::Left, cx),
        Some(history.entity_id()),
        "clicking the entry activates its panel"
    );
    let panel_focused = cx.update(|window, cx| {
        let focus_handle = history.read_with(cx, |panel, cx| panel.focus_handle(cx));
        focus_handle.is_focused(window)
    });
    assert!(
        panel_focused,
        "clicking the entry moves keyboard focus into its panel"
    );
}

#[gpui::test]
fn re_registering_a_panel_id_replaces_the_entry_instead_of_duplicating_it(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let first = test_panel(cx, "collections", "Collections", DockPosition::Left);
    register_panel(&workspace, first, cx);
    let replacement = test_panel(cx, "collections", "Collections (new)", DockPosition::Left);
    register_panel(&workspace, replacement.clone(), cx);
    draw_window(cx);

    let panel_count = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace.left_dock().read(cx).panels().len()
        })
    });
    assert_eq!(
        panel_count, 1,
        "re-registering the same panel id replaces the entry instead of duplicating it"
    );
    assert!(
        entry_bounds(cx, "collections").is_some(),
        "the replaced entry still renders once"
    );
    assert_eq!(
        active_panel_id(&workspace, DockPosition::Left, cx),
        Some(replacement.entity_id()),
        "the replacement panel takes the entry's place"
    );
}
