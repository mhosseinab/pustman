//! Pointer-leave dismissal of the `{{variable}}` hover (REQ-VAR-03, #194): the popover
//! belongs to the placeholder under the pointer, so a pointer that leaves the field for
//! anywhere else in the window closes it on the next move — no further move over the
//! field needed. Driven the way a user drives it: real mouse moves.

use std::collections::BTreeMap;

use api_model::{HttpRequest, Variable, VariableKind};
use gpui::{
    Bounds, Entity, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px,
};
use ui::test_support::{draw_window, init_test};
use variables::{ScopeKind, ScopeStack};

use super::{SecretSources, VARIABLE_HOVER_TARGET_SELECTOR};
use crate::RequestEditor;

/// The scopes in effect: one environment holding a plain `token` — enough to hover.
fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![Variable {
            key: String::from("token"),
            value: Some(String::from("Ada")),
            kind: VariableKind::Default,
            enabled: true,
            description: None,
            extra: BTreeMap::new(),
        }],
    );
    scopes
}

/// The draft the hover runs over: a URL that is one placeholder, so the pointer at the
/// field's first character is inside its span.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("{{token}}");
    request
}

/// A request tab on the app's real init order, with the fixture scopes in effect and
/// the window painted (hitboxes register during a paint).
fn open_editor(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_scopes(scopes(), cx);
            editor.set_secret_sources(SecretSources::default(), cx);
        });
    });
    draw_window(cx);
    (editor, cx)
}

/// Hover the URL field the user's way: the pointer moves onto the field's first
/// character, which the URL fixture puts inside `{{token}}`'s span. Returns the field's
/// bounds so a test can move the pointer off them.
fn hover_url(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let bounds = cx
        .debug_bounds(VARIABLE_HOVER_TARGET_SELECTOR)
        .expect("the URL field's hover wrapper paints");
    cx.simulate_mouse_move(
        point(bounds.left() + px(2.), bounds.top() + px(2.)),
        None::<MouseButton>,
        Modifiers::default(),
    );
    cx.run_until_parked();
    draw_window(cx);
    bounds
}

/// Whether the hover is open, exactly as `render` draws it.
fn hover_open(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor.url_bar().read(cx).hover().read(cx).hovered()
        })
    })
    .is_some()
}

#[gpui::test]
fn pointer_leaving_the_field_dismisses_the_hover(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    let bounds = hover_url(cx);
    assert!(
        hover_open(&editor, cx),
        "hovering {{token}} opens the hover (REQ-VAR-03)"
    );

    // The pointer leaves the field for the space below it and never moves over the
    // field again — the popover used to stay open until it did.
    cx.simulate_mouse_move(
        point(bounds.left(), bounds.bottom() + px(64.)),
        None::<MouseButton>,
        Modifiers::default(),
    );
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        !hover_open(&editor, cx),
        "a pointer that left the field dismisses the hover (REQ-VAR-03)"
    );
}

#[gpui::test]
fn pointer_over_the_popover_keeps_the_hover_open(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    let bounds = hover_url(cx);
    assert!(
        hover_open(&editor, cx),
        "hovering {{token}} opens the hover (REQ-VAR-03)"
    );

    // The pointer travels to the popover surface — anchored at the field's bottom-left
    // corner, so a point just inside its top-left is on it. The reveal toggle lives on
    // that surface: the popover must not dismiss under the pointer.
    cx.simulate_mouse_move(
        point(bounds.left() + px(20.), bounds.bottom() + px(20.)),
        None::<MouseButton>,
        Modifiers::default(),
    );
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        hover_open(&editor, cx),
        "the popover stays open while the pointer is on it"
    );
}
