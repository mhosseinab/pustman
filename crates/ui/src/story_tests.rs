//! The story proves the whole design system draws: every prelude component lays out in
//! both themes, and the story's button keeps the keyboard contract every button has —
//! Enter and Space activate it once per press (`components/button_tests.rs`).

use gpui::TestAppContext;
use theme::{ActiveTheme, Appearance, ThemeStore};

use crate::story::{ComponentStory, STORY_SECTIONS};
use crate::test_support::{draw_window, init_test, press_key};

#[gpui::test]
fn all_components_render_in_both_themes(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (story, cx) = cx.add_window_view(|_, cx| ComponentStory::new(cx));

    for appearance in [Appearance::Light, Appearance::Dark] {
        cx.update(|_window, cx| ThemeStore::set_system_appearance(cx, appearance));
        draw_window(cx);

        assert_eq!(
            cx.read(|cx| cx.theme().appearance()),
            appearance,
            "the active theme follows the system appearance"
        );
        for section in STORY_SECTIONS {
            assert!(
                cx.debug_bounds(section).is_some(),
                "{section} did not lay out in the {appearance:?} theme"
            );
        }
    }

    // Rendering the story is free of side effects: nothing activates on its own.
    let button_clicks = cx.read(|cx| story.read_with(cx, |story, _| story.button_clicks()));
    assert_eq!(button_clicks, 0, "rendering the story activates nothing");
}

#[gpui::test]
fn button_activates_on_enter_and_space(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (story, cx) = cx.add_window_view(|_, cx| ComponentStory::new(cx));
    draw_window(cx);

    let focus_handle = cx.read(|cx| story.read_with(cx, |story, _| story.button_focus_handle()));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    // The button's key handlers register when the focused frame paints.
    draw_window(cx);

    press_key(cx, "enter");
    let button_clicks = cx.read(|cx| story.read_with(cx, |story, _| story.button_clicks()));
    assert_eq!(button_clicks, 1, "Enter activates exactly once");

    press_key(cx, "space");
    let button_clicks = cx.read(|cx| story.read_with(cx, |story, _| story.button_clicks()));
    assert_eq!(button_clicks, 2, "Space activates exactly once");
}
