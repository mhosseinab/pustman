//! The row menu's Copy path (#146): the menu of a header row offers one entry that puts
//! `Name: value` on the clipboard (REQ-RESP-07) — the row the menu was opened for, with
//! duplicate field names kept apart by occurrence. The menu opens from the keyboard too
//! (`headers_view.rs`): Enter on the focused row, then Enter on the menu's entry.

use std::ops::Deref;

use gpui::{
    Bounds, ElementId, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, px, size,
};
use http_engine::Header;
use ui::components::context_menu::MENU_SELECTOR;
use ui::test_support::{draw_window, init_test, press_key};

use super::HeadersView;
use crate::child_id;

#[gpui::test]
fn test_the_row_menu_copies_the_row_it_was_opened_for_as_name_and_value(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let window = cx.open_window(size(px(800.), px(300.)), |_, cx| HeadersView::new(cx));
    let view = window
        .entity(cx)
        .expect("the window holds the headers view");
    let mut cx = VisualTestContext::from_window(*window.deref(), cx);
    cx.update(|window, _cx| window.activate_window());
    view.update(&mut cx, |view, cx| {
        view.set_headers(
            &[
                Header::new("Set-Cookie", "sid=one"),
                Header::new("Content-Type", "application/json"),
                Header::new("Set-Cookie", "sid=two"),
            ],
            cx,
        )
    });
    draw_window(&mut cx);

    // Click the last of the two `Set-Cookie` rows to focus it, then open its menu from
    // the keyboard.
    let row = row_bounds(&view, 2, &mut cx).expect("the third row is painted");
    cx.simulate_click(row.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(&mut cx);
    press_key(&mut cx, "enter");
    cx.run_until_parked();
    draw_window(&mut cx);

    assert!(
        menu_bounds(&mut cx).is_some(),
        "Enter on the focused row opens its Copy menu"
    );

    // Enter runs the menu's one entry.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(&mut cx);

    assert_eq!(
        clipboard_text(&mut cx).as_deref(),
        Some("Set-Cookie: sid=two"),
        "Copy puts the row's name and value on the clipboard — the occurrence the menu \
         was opened for, not the first row of the same name"
    );
    assert!(
        !row_menu_open(&view, &cx),
        "running Copy dismisses the menu through its Dismissed event"
    );
    let row_focused = cx.update(|window, cx| {
        view.read(cx)
            .row_focus
            .get(2)
            .is_some_and(|focus| focus.is_focused(window))
    });
    assert!(
        row_focused,
        "dismissal hands focus back to the row the menu was opened from"
    );
}

/// Where the row showing header field `index` is painted. The selector is the row's item
/// key, the same string [`ui::components::list::VirtualList`] registers
/// (`components/list.rs`).
fn row_bounds(
    view: &Entity<HeadersView>,
    index: usize,
    cx: &mut VisualTestContext,
) -> Option<Bounds<Pixels>> {
    let key_name = view.read_with(cx, |view, _| {
        view.rows().get(index).map(|row| row.key_name())
    })?;
    let base = ElementId::from(("headers-view", view.entity_id()));
    cx.debug_bounds(leak(child_id(&base, key_name).to_string()))
}

/// Where the open row menu is painted — its container's test hook ([`MENU_SELECTOR`]),
/// `None` when no menu is drawn.
fn menu_bounds(cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(leak(MENU_SELECTOR.to_owned()))
}

/// Whether the table still holds an open row menu.
fn row_menu_open(view: &Entity<HeadersView>, cx: &VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.row_menu.is_some())
}

/// What the clipboard holds, if anything.
fn clipboard_text(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

/// `VisualTestContext::debug_bounds` takes a `&'static str`, so selectors are leaked —
/// fine for a test's short life (the `timeline_tests.rs` pattern).
fn leak(selector: String) -> &'static str {
    selector.leak()
}
