//! The Headers tab scrolls its rows inside the editor half of the split (#207): the
//! Automatic Headers block sits at the bottom of the tab, so on a short request pane
//! its last rows must scroll into view instead of clipping at the pane's bottom edge
//! (shot 32: "Send User-Agent" cut off). The Settings tab gained the same scroll
//! earlier; this is the Headers tab's side of that fix.
//!
//! `Decision:` the scroll lives in the tab, not in the split's top pane — the shape
//! `settings_tab` and `auth_tab` already use — so the URL row and the request tab strip
//! stay pinned while the tab's rows scroll under them (the invariant
//! `wire_settings_tab_layout` proves for Settings). `response_split`'s `overflow_hidden`
//! stays: it is the clip boundary the tabs scroll inside. Spec: S02/R02 keeps the
//! split resizable; S03/R03 keeps every headers row usable in small windows (the
//! shared contract's "long content … and small windows remain usable").

use api_model::HttpRequest;
use gpui::{
    Entity, Modifiers, MouseButton, Point, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext,
    TouchPhase, VisualTestContext, px, size,
};
use ui::test_support::{draw_window, init_test};

use crate::REQUEST_TAB_CONTENT_SELECTOR;
use crate::RequestEditor;

/// The draft the tab shows: a plain GET with a parseable URL, so the tab's dry
/// [`http_engine::prepare`] lists the request's auto headers.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/items");
    request
}

/// A request tab on the app's real init order, on a window short enough that the
/// request half of the split cannot show the whole Automatic Headers block at once —
/// the shape of the hand check that found this.
fn open_editor(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.simulate_resize(size(px(1200.), px(520.)));
    cx.run_until_parked();
    draw_window(cx);
    (editor, cx)
}

/// Open the Headers tab the way a user does: click it in the request tab strip.
fn open_headers_tab(cx: &mut VisualTestContext) {
    let tab = cx
        .debug_bounds("request-tab-headers")
        .expect("the Headers tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The debug key the bottom-most Automatic Headers row paints under (`render_auto_row`
/// spells it `auto-row-{name}`): the row #207 saw clipped.
fn last_auto_row_selector(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> &'static str {
    let name = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .headers
                .read(cx)
                .auto_rows()
                .last()
                .map(|row| row.name.clone())
        })
    });
    let name = name.expect("the fixture request lists auto headers: its URL parses");
    format!("auto-row-{name}").leak()
}

#[gpui::test]
fn test_automatic_headers_rows_scroll_into_view_instead_of_clipping(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    open_headers_tab(cx);

    let row_selector = last_auto_row_selector(&editor, cx);
    let row_before = cx
        .debug_bounds(row_selector)
        .expect("the last Automatic Headers row is painted, clipped or not");
    let pane_before = cx
        .debug_bounds(REQUEST_TAB_CONTENT_SELECTOR)
        .expect("the request tab's content pane is painted");
    assert!(
        row_before.bottom() > pane_before.bottom(),
        "fixture: the last Automatic Headers row starts clipped below the editor half of \
         the split ({row_before:?} over {pane_before:?})"
    );

    // The gesture that must reach it: a wheel scroll over the tab.
    cx.simulate_mouse_move(pane_before.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_event(ScrollEvent {
        position: pane_before.center(),
        delta: ScrollDelta::Pixels(Point::new(px(0.), px(-1200.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    });
    cx.run_until_parked();
    draw_window(cx);

    let row_after = cx
        .debug_bounds(row_selector)
        .expect("the last Automatic Headers row is painted after the scroll");
    let pane_after = cx
        .debug_bounds(REQUEST_TAB_CONTENT_SELECTOR)
        .expect("the request tab's content pane is painted after the scroll");
    assert!(
        row_after.origin.y < row_before.origin.y,
        "scrolling the Headers tab moves its rows up"
    );
    assert!(
        row_after.bottom() <= pane_after.bottom(),
        "the last Automatic Headers row scrolls fully into view instead of clipping at the \
         bottom of the editor half ({row_after:?} in {pane_after:?})"
    );
}
