//! The Pretty/Raw/Hex/Preview switch sits under the Body/Headers/… section row, so it is
//! drawn as a compact segmented control, not as a second underlined tab row.

use std::sync::Arc;

use gpui::TestAppContext;
use http_engine::{Response, ResponseBody};
use ui::SEGMENTED_TRACK_SELECTOR;
use ui::test_support::{draw_window, init_test};

use crate::body_view::BodyView;

#[gpui::test]
fn test_the_body_mode_switch_is_a_segmented_control(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    let response = Response::new(200, "OK", ResponseBody::Bytes(b"hello".to_vec()));
    view.update(cx, |view, cx| view.set_body(Arc::new(response), cx));
    cx.run_until_parked();
    draw_window(cx);

    let track = cx
        .debug_bounds(SEGMENTED_TRACK_SELECTOR)
        .expect("the mode switch draws its modes in one segmented track");
    assert!(
        track.size.height < gpui::px(40.),
        "the switch is more compact than a section tab row: {track:?}"
    );
}
