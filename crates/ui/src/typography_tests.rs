//! [`TextSize::Ui`] and [`TextSize::Editor`] resolve through the registered
//! [`super::FontSizesSetting`] view. Before `ui` registered that view they panicked with
//! "settings type ... is not registered", so these run the real init order — `settings →
//! theme → ui`, the order `main.rs` runs it in — and pin both sizes to the configured
//! values.

use gpui::{TestAppContext, px};
use settings::SettingsStore;

use crate::test_support::init_test;
use crate::typography::TextSize;

#[gpui::test]
fn text_sizes_resolve_through_the_registered_setting(cx: &mut TestAppContext) {
    cx.update(init_test);

    // What a user who changes nothing gets: the documented defaults (REQ-UI-08).
    cx.update(|cx| {
        assert_eq!(TextSize::Ui.pixels_absolute(cx), px(16.));
        assert_eq!(TextSize::Editor.pixels_absolute(cx), px(14.));
    });

    // The font sizes the user configured win, and nothing re-registers the view for the
    // change to be picked up.
    cx.update(|cx| {
        SettingsStore::reload(cx, Some(r#"{"ui_font_size": 20, "buffer_font_size": 12}"#))
            .expect("the font sizes parse");
    });
    cx.update(|cx| {
        assert_eq!(TextSize::Ui.pixels_absolute(cx), px(20.));
        assert_eq!(TextSize::Editor.pixels_absolute(cx), px(12.));
    });
}
