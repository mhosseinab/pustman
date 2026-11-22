//! The row menu must outlive the row it opened from (#144): the headers table lays its
//! rows out through [`ui::components::list::VirtualList`], which unmounts the rows the
//! viewport cannot show — a menu drawn inside its row is dropped the moment that row
//! scrolls away, hidden with no `Dismissed` event while `row_menu` and the menu's focus
//! stay live. The menu is an overlay of the whole table instead, so this test scrolls its
//! row out from under it and then runs the menu anyway.

use std::ops::Deref;

use gpui::{
    Bounds, ElementId, Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent,
    ScrollSource, TestAppContext, TouchPhase, VisualTestContext, px, size,
};
use http_engine::Header;
use ui::components::context_menu::MENU_SELECTOR;
use ui::test_support::{draw_window, init_test};

use super::HeadersView;
use crate::child_id;

#[gpui::test]
fn test_scrolling_the_row_out_of_view_keeps_its_copy_menu_open(cx: &mut TestAppContext) {
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
    view.update(&mut cx, |view, cx| view.set_headers(&headers(), cx));
    draw_window(&mut cx);

    // Right-click the first row: its "Copy" menu opens over the table.
    let first_row =
        row_bounds(&view, 0, &mut cx).expect("the first row is painted before the scroll");
    right_click(
        Point::new(first_row.origin.x + px(300.), first_row.center().y),
        &mut cx,
    );
    assert!(
        menu_bounds(&mut cx).is_some(),
        "the row menu is painted when it opens"
    );

    // Scroll the table far enough that the first row leaves the viewport — from a point
    // the menu does not cover, so the wheel reaches the list.
    let scroll_at = Point::new(first_row.right() - px(40.), first_row.center().y);
    cx.simulate_mouse_move(scroll_at, None::<MouseButton>, Modifiers::none());
    cx.simulate_event(ScrollEvent {
        position: scroll_at,
        delta: ScrollDelta::Pixels(Point::new(px(0.), px(-900.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    });
    cx.run_until_parked();
    draw_window(&mut cx);

    assert!(
        row_bounds(&view, 0, &mut cx).is_none(),
        "the test really does scroll the first row out of the viewport"
    );
    assert!(
        menu_bounds(&mut cx).is_some(),
        "scrolling the row away leaves the menu on screen: it goes away through a \
         Dismissed event, never by being scrolled out of existence"
    );
    assert!(
        row_menu_open(&view, &cx),
        "the menu is still the one open row menu, not a drawn ghost"
    );

    // And it is still the live one: its Copy entry runs, dismisses through the menu's
    // own event and hands focus back to its row.
    let entry = entry_bounds(&mut cx).expect("the Copy entry is painted with the menu");
    cx.simulate_click(entry.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(&mut cx);

    assert_eq!(
        clipboard_text(&mut cx).as_deref(),
        Some("x-header-0: value 0"),
        "the open menu still copies its row after the row scrolled away"
    );
    assert!(
        !row_menu_open(&view, &cx),
        "running the entry dismisses the menu through its Dismissed event"
    );
    let row_focused = cx.update(|window, cx| {
        view.read(cx)
            .row_focus
            .first()
            .is_some_and(|focus| focus.is_focused(window))
    });
    assert!(
        row_focused,
        "dismissal hands focus back to the row the menu was opened from"
    );
}

/// Enough header fields that the table is taller than the test window and scrolls.
fn headers() -> Vec<Header> {
    (0..60)
        .map(|row| Header::new(format!("x-header-{row}"), format!("value {row}")))
        .collect()
}

/// Where the row showing header field `index` is painted — `None` once it scrolls out of
/// the viewport and [`ui::components::list::VirtualList`] unmounts it. The selector is
/// the row's item key, the same string the list registers (`components/list.rs`).
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

/// Whether the table still holds an open row menu.
fn row_menu_open(view: &Entity<HeadersView>, cx: &VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.row_menu.is_some())
}

/// Where the open row menu is painted — its container's test hook ([`MENU_SELECTOR`]),
/// `None` when no menu is drawn.
fn menu_bounds(cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(leak(MENU_SELECTOR.to_owned()))
}

/// Where the menu's "Copy" entry is painted (its test hook is the entry's key,
/// `components/context_menu.rs`).
fn entry_bounds(cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(leak(String::from("Copy")))
}

/// Right-click `at`, the gesture that opens a row's menu.
fn right_click(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw_window(cx);
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
