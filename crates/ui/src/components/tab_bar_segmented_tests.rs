//! Segmented tabs (Pretty/Raw/Hex/Preview inside the response Body tab): a second level
//! of tabs under a section row reads as a compact switch — chips in one rounded track —
//! not as a second underlined tab row stacked under the first.

use gpui::{Entity, Focusable, TestAppContext, VisualTestContext, px};

use crate::components::tab::Tab;
use crate::components::tab_bar::{SEGMENTED_TRACK_SELECTOR, TabBar};
use crate::test_support::{draw_window, init_test, press_key};

fn tabs() -> Vec<Tab> {
    vec![
        Tab::new("pretty", "Pretty"),
        Tab::new("raw", "Raw"),
        Tab::new("hex", "Hex"),
    ]
}

fn segmented_bar(cx: &mut TestAppContext) -> (Entity<TabBar>, &mut VisualTestContext) {
    cx.update(init_test);
    let (bar, cx) = cx.add_window_view(|_, cx| TabBar::new(cx).segmented().with_tabs(tabs()));
    draw_window(cx);
    (bar, cx)
}

#[gpui::test]
fn test_a_segmented_bar_is_shorter_than_a_section_row_and_has_no_underline(
    cx: &mut TestAppContext,
) {
    cx.update(init_test);
    let (_bar, section_cx) =
        cx.add_window_view(|_, cx| TabBar::new(cx).section().with_tabs(tabs()));
    draw_window(section_cx);
    let section_tab = section_cx.debug_bounds("pretty").expect("section tab");

    let (_bar, cx) = segmented_bar(cx);
    let segment = cx.debug_bounds("pretty").expect("segment rendered");
    assert!(
        segment.size.height <= px(24.),
        "a segment is a compact chip: {:?}",
        segment.size.height
    );
    assert!(
        segment.size.height < section_tab.size.height,
        "it is shorter than a section tab, so the two rows don't read as one kind"
    );
    assert!(
        cx.debug_bounds("pretty-underline").is_none(),
        "the selected segment is filled, not underlined"
    );
}

#[gpui::test]
fn test_segments_sit_inset_inside_one_track(cx: &mut TestAppContext) {
    let (_bar, cx) = segmented_bar(cx);
    let track = cx
        .debug_bounds(SEGMENTED_TRACK_SELECTOR)
        .expect("the segments share a track");
    for selector in ["pretty", "raw", "hex"] {
        let segment = cx.debug_bounds(selector).expect("segment rendered");
        assert!(
            segment.top() > track.top() && segment.bottom() < track.bottom(),
            "{selector} is inset from the track's top and bottom: {segment:?} in {track:?}"
        );
        assert!(
            segment.left() > track.left() - px(0.5) && segment.right() <= track.right(),
            "{selector} lies inside the track"
        );
    }
}

#[gpui::test]
fn test_arrows_still_move_a_segmented_selection(cx: &mut TestAppContext) {
    let (bar, cx) = segmented_bar(cx);
    let focus_handle = cx.read(|cx| bar.read_with(cx, |bar, cx| bar.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);

    press_key(cx, "right");
    assert_eq!(
        cx.read(|cx| bar.read(cx).selected()),
        1,
        "Right selects Raw"
    );
    press_key(cx, "left");
    assert_eq!(cx.read(|cx| bar.read(cx).selected()), 0, "Left goes back");
}
