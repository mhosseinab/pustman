//! The modal layer hosts one modal at a time: it takes keyboard focus while open, gives
//! focus back when it goes — on Escape, on a click outside it, or on its own
//! [`DismissEvent`] — and opening a second modal replaces the first. A stub that opened
//! modals and never closed them fails here.

use gpui::{
    App, Bounds, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    Modifiers, ParentElement, Pixels, Point, Render, TestAppContext, VisualTestContext, Window,
    div,
};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test, press_key};

use crate::modal_layer::MODAL_CONTAINER_ID;
use crate::{ModalView, Workspace, init};

/// A minimal modal: takes focus, draws one line, and closes when its caller says so.
/// A real dialog would wear the `ui::Modal` chrome; the layer only hosts a view.
struct TestModal {
    focus_handle: FocusHandle,
    title: &'static str,
}

impl TestModal {
    fn new(title: &'static str, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            title,
        }
    }
}

impl Render for TestModal {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .child(Label::new(self.title))
    }
}

impl Focusable for TestModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for TestModal {}

impl ModalView for TestModal {}

/// A second modal type — one the layer must swap the open [`TestModal`] for.
struct OtherModal {
    focus_handle: FocusHandle,
}

impl OtherModal {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for OtherModal {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .child(Label::new("Other modal"))
    }
}

impl Focusable for OtherModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for OtherModal {}

impl ModalView for OtherModal {}

/// A workspace in its own window, on the app's real init order and the real default
/// keymap (a local copy of `workspace_tests.rs`'s helper — existing tests stay read-only).
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

/// Open a [`TestModal`] through the workspace, the way a screen would.
fn open_test_modal(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |_window, cx| TestModal::new("Test modal", cx));
        });
    });
}

/// Whether the modal layer is showing a modal.
fn modal_is_open(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| workspace.read_with(cx, |workspace, _| workspace.modal_layer().is_open()))
}

/// The open [`TestModal`], if the layer is hosting one.
fn active_test_modal(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Option<Entity<TestModal>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<TestModal>()
        })
    })
}

/// Whether `outer` fully contains `inner` (the `workspace_tests.rs` shape).
fn contains_bounds(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.origin.x + inner.size.width <= outer.origin.x + outer.size.width
        && inner.origin.y + inner.size.height <= outer.origin.y + outer.size.height
}

#[gpui::test]
fn modal_opens_focused_and_escape_restores_focus(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);

    // Somewhere the user was before the modal opened; focus must come back here.
    let previous_focus = cx.update(|window, cx| {
        let focus_handle = workspace.read_with(cx, |workspace, cx| workspace.focus_handle(cx));
        window.focus(&focus_handle, cx);
        focus_handle
    });

    open_test_modal(&workspace, cx);
    draw_window(cx);

    let modal = active_test_modal(&workspace, cx).expect("the modal is open");
    assert!(modal_is_open(&workspace, cx), "the modal layer is open");
    let modal_focus = modal.read_with(cx, |modal, cx| modal.focus_handle(cx));
    let (modal_focused, previous_focused) = cx.update(|window, _| {
        (
            modal_focus.is_focused(window),
            previous_focus.is_focused(window),
        )
    });
    assert!(modal_focused, "the modal takes keyboard focus");
    assert!(!previous_focused, "focus left where the user was");

    press_key(cx, "escape");
    draw_window(cx);

    assert!(!modal_is_open(&workspace, cx), "Escape closes the modal");
    assert!(
        active_test_modal(&workspace, cx).is_none(),
        "the dismissed modal is gone"
    );
    let (modal_focused, previous_focused) = cx.update(|window, _| {
        (
            modal_focus.is_focused(window),
            previous_focus.is_focused(window),
        )
    });
    assert!(!modal_focused, "the dismissed modal holds no focus");
    assert!(
        previous_focused,
        "focus returns to where it was before the modal opened"
    );
}

#[gpui::test]
fn clicking_outside_the_modal_dismisses_it(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    open_test_modal(&workspace, cx);
    draw_window(cx);

    // The modal is centered, so the window's top-left corner is outside it.
    let outside = Point::new(px(4.), px(4.));
    let window_bounds = cx.update(|window, _| window.bounds());
    let container = cx
        .debug_bounds(MODAL_CONTAINER_ID)
        .expect("the modal renders in its container");
    assert!(
        contains_bounds(window_bounds, container),
        "the modal sits inside the window"
    );
    assert!(
        !container.contains(&outside),
        "the chosen click lands outside the modal"
    );

    cx.simulate_click(outside, Modifiers::none());
    draw_window(cx);

    assert!(
        !modal_is_open(&workspace, cx),
        "a click outside the modal dismisses it"
    );
    assert!(
        cx.debug_bounds(MODAL_CONTAINER_ID).is_none(),
        "the dismissed modal renders no more"
    );
}

#[gpui::test]
fn opening_a_second_modal_replaces_the_first(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    open_test_modal(&workspace, cx);
    draw_window(cx);
    assert!(modal_is_open(&workspace, cx), "the first modal is open");

    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |_window, cx| OtherModal::new(cx));
        });
    });
    draw_window(cx);

    assert!(modal_is_open(&workspace, cx), "the second modal is open");
    assert!(
        active_test_modal(&workspace, cx).is_none(),
        "the first modal is gone"
    );
    let second_is_open = cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<OtherModal>()
                .is_some()
        })
    });
    assert!(
        second_is_open,
        "the layer holds the second modal — one at a time"
    );
}

#[gpui::test]
fn modal_emitting_dismiss_event_closes_it(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    open_test_modal(&workspace, cx);
    draw_window(cx);

    let modal = active_test_modal(&workspace, cx).expect("the modal is open");
    cx.update(|_, cx| modal.update(cx, |_modal, cx| cx.emit(DismissEvent)));
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        !modal_is_open(&workspace, cx),
        "a modal that emits DismissEvent closes"
    );
    assert!(
        active_test_modal(&workspace, cx).is_none(),
        "the dismissed modal is gone"
    );
}
