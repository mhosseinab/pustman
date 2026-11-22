//! The numeric field's parse note (#181, S28c): "Enter a whole number…" shows while
//! the text is not a whole number and clears the moment the field reads as one again —
//! including the fill-echo path, where one edit lands back on the value the field
//! already displays.

use api_model::HttpRequest;
use gpui::{Entity, Focusable, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::{NumericField, SettingsTab};
use crate::RequestEditor;

/// A draft GET — the Settings tab opens the same on any method.
fn request() -> HttpRequest {
    crate::new_request::fresh_request()
}

/// A request tab on the app's real init order, with the Settings tab open.
fn open_settings_tab(cx: &mut TestAppContext) -> (Entity<SettingsTab>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);

    let tab = cx
        .debug_bounds("request-tab-settings")
        .expect("the Settings tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    let settings_tab = cx.read(|cx| editor.read(cx).settings_tab().clone());
    (settings_tab, cx)
}

/// The select-all keystroke on this platform, exactly as `text_input::init` binds it.
fn select_all_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

/// Replace the field's whole text, the way a user does: select all, then type over it.
fn replace_text(
    field: NumericField,
    tab: &Entity<SettingsTab>,
    cx: &mut VisualTestContext,
    text: &str,
) {
    focus_field(field, tab, cx);
    cx.simulate_keystrokes(select_all_keys(cx));
    cx.simulate_input(text);
    cx.run_until_parked();
    draw_window(cx);
}

/// Put the caret in the field, the way a user starts an edit.
fn focus_field(field: NumericField, tab: &Entity<SettingsTab>, cx: &mut VisualTestContext) {
    let input = cx.read(|cx| tab.read(cx).numeric_input(field).clone());
    cx.update(|window, cx| {
        let focus_handle = input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
}

/// The text the field holds and the value the tab shows for it.
fn field_state(
    field: NumericField,
    tab: &Entity<SettingsTab>,
    cx: &VisualTestContext,
) -> (String, String) {
    cx.read(|cx| {
        let tab = tab.read(cx);
        (tab.numeric_input(field).read(cx).text(), field.display(tab))
    })
}

/// The debug key the field's parse note paints under.
fn note_selector(field: NumericField) -> String {
    format!("{}-note", field.key())
}

/// Whether anything paints under this debug key (`m1_layout_fixes_tests.rs`'s shape).
fn shown(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

#[gpui::test]
fn test_the_parse_note_clears_when_an_edit_lands_on_the_displayed_value(cx: &mut TestAppContext) {
    let (tab, cx) = open_settings_tab(cx);

    replace_text(NumericField::TimeoutMs, &tab, cx, "12x");
    assert!(
        shown(cx, &note_selector(NumericField::TimeoutMs)),
        "an edit that is not a whole number shows the note"
    );

    // The user corrects the trailing "x" away. The keystroke's edit lands back on the
    // value the field displays — the fill-echo shape, where the edit is a no-op for
    // the setting but not for the note.
    let (text, displayed) = field_state(NumericField::TimeoutMs, &tab, cx);
    assert_eq!(
        text,
        format!("{displayed}x"),
        "the unparseable edit is the displayed value plus one stray character, so the \
         correction lands back on the displayed value"
    );
    focus_field(NumericField::TimeoutMs, &tab, cx);
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        !shown(cx, &note_selector(NumericField::TimeoutMs)),
        "the note clears as soon as the field reads as the displayed value again — the \
         edit is a no-op for the setting, but the note the unparseable edit left is \
         stale now"
    );
}

#[gpui::test]
fn test_the_parse_note_clears_when_a_whole_number_is_typed(cx: &mut TestAppContext) {
    let (tab, cx) = open_settings_tab(cx);

    replace_text(NumericField::MaxRedirects, &tab, cx, "many");
    assert!(
        shown(cx, &note_selector(NumericField::MaxRedirects)),
        "an edit that is not a whole number shows the note"
    );

    replace_text(NumericField::MaxRedirects, &tab, cx, "7");
    assert!(
        !shown(cx, &note_selector(NumericField::MaxRedirects)),
        "a whole number clears the note"
    );
}
