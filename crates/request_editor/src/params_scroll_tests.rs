//! The Params tab scrolls its rows inside the editor half of the split, like Headers
//! (#207): on a short pane a long query table's last row and its "Add row" scroll into
//! view instead of clipping at the pane's bottom edge.

use api_model::{HttpRequest, KeyValue};
use gpui::{
    Modifiers, MouseButton, Point, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext,
    TouchPhase, px, size,
};
use ui::test_support::{draw_window, init_test};

use crate::REQUEST_TAB_CONTENT_SELECTOR;
use crate::RequestEditor;
use crate::key_value_table::ADD_ROW_SELECTOR;

fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.test/orders");
    request.params = (0..14)
        .map(|index| KeyValue {
            key: format!("key{index}"),
            value: Some(format!("value{index}")),
            description: None,
            enabled: true,
            extra: Default::default(),
        })
        .collect();
    request
}

#[gpui::test]
fn test_the_last_params_row_scrolls_into_view_on_a_short_pane(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (_editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.simulate_resize(size(px(1200.), px(520.)));
    cx.run_until_parked();
    draw_window(cx);

    let add_before = cx
        .debug_bounds(ADD_ROW_SELECTOR)
        .expect("the Add row control is painted, clipped or not");
    let pane = cx
        .debug_bounds(REQUEST_TAB_CONTENT_SELECTOR)
        .expect("the request tab's content pane is painted");
    assert!(
        add_before.bottom() > pane.bottom(),
        "fixture: the table starts taller than the pane ({add_before:?} over {pane:?})"
    );

    cx.simulate_mouse_move(pane.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_event(ScrollEvent {
        position: pane.center(),
        delta: ScrollDelta::Pixels(Point::new(px(0.), px(-2000.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    });
    cx.run_until_parked();
    draw_window(cx);

    let add_after = cx
        .debug_bounds(ADD_ROW_SELECTOR)
        .expect("the Add row control is painted after the scroll");
    let pane_after = cx
        .debug_bounds(REQUEST_TAB_CONTENT_SELECTOR)
        .expect("the pane is painted after the scroll");
    assert!(
        add_after.bottom() <= pane_after.bottom(),
        "the last row and Add row scroll fully into view ({add_after:?} in {pane_after:?})"
    );
}
