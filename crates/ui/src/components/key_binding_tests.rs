use gpui::{Keystroke, OperatingSystem};

use super::keystroke_label;

fn label(keystroke: &str, os: OperatingSystem) -> String {
    let keystroke = Keystroke::parse_with_os(keystroke, os).expect("the test keystroke must parse");
    keystroke_label(&keystroke.modifiers, &keystroke.key, os)
}

#[test]
fn test_linux_hints_read_as_title_case_keys_joined_by_plus() {
    let os = OperatingSystem::Linux;
    assert_eq!(label("ctrl-w", os), "Ctrl+W");
    assert_eq!(label("ctrl-tab", os), "Ctrl+Tab");
    assert_eq!(label("ctrl-shift-tab", os), "Ctrl+Shift+Tab");
    assert_eq!(label("ctrl-alt-c", os), "Ctrl+Alt+C");
    assert_eq!(label("ctrl-\\", os), "Ctrl+\\");
    assert_eq!(label("ctrl-enter", os), "Ctrl+Enter");
    assert_eq!(label("super-k", os), "Super+K");
}

#[test]
fn test_named_keys_keep_their_inner_capitals() {
    let os = OperatingSystem::Linux;
    assert_eq!(label("pagedown", os), "PageDown");
    assert_eq!(label("pageup", os), "PageUp");
    assert_eq!(label("f5", os), "F5");
    assert_eq!(label("escape", os), "Escape");
}

#[test]
fn test_windows_names_the_platform_key_win() {
    assert_eq!(label("win-k", OperatingSystem::Windows), "Win+K");
    assert_eq!(
        label("ctrl-shift-p", OperatingSystem::Windows),
        "Ctrl+Shift+P"
    );
}

#[test]
fn test_mac_keeps_the_native_modifier_symbols() {
    let os = OperatingSystem::Mac;
    let expected = Keystroke::parse_with_os("cmd-shift-p", os)
        .expect("the test keystroke must parse")
        .display(os)
        .to_string();
    assert_eq!(label("cmd-shift-p", os), expected);
}
