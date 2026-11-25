//! A disabled button stays legible: a disabled Filled button drops the accent fill for
//! the neutral disabled fill and the disabled text colour, with no opacity on top. Grey
//! text over the accent at reduced opacity cannot be read.

use gpui::TestAppContext;
use theme::ActiveTheme;

use super::ButtonStyle;
use crate::test_support::init_test;

#[gpui::test]
fn disabled_filled_button_uses_the_neutral_disabled_fill_and_text(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        let colors = cx.theme().colors();
        assert_eq!(
            ButtonStyle::Filled.fill(true, cx),
            Some(colors.element_disabled),
            "a disabled Filled button sits on the neutral disabled fill"
        );
        assert_eq!(
            ButtonStyle::Filled.label_color(true, cx),
            colors.text_disabled,
            "its label takes the disabled text colour"
        );
        assert!(
            !ButtonStyle::Filled.fades_when_disabled(),
            "no extra opacity is multiplied over the disabled colours"
        );
        assert_eq!(
            ButtonStyle::Filled.fill(false, cx),
            Some(colors.accent_background),
            "an enabled Filled button keeps the accent fill"
        );
    });
}
