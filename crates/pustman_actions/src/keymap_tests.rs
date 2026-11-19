use super::{KeymapPlatform, load_keymap, parse_keymap_entries};
use gpui::TestAppContext;

/// Every keybinding the PRD declares: the "Default keybindings" table of §3, plus `Esc` to
/// cancel an in-flight request (REQ-HTTP-12) and `Ctrl-F` to find in the response body
/// (REQ-RESP-03). Each row is (action, context, Linux/Windows keystroke, macOS keystroke);
/// the platform modifiers follow PRD §3: Cmd on macOS, Ctrl on Linux and Windows.
const PRD_KEYBINDINGS: &[(&str, &str, &str, &str)] = &[
    ("request::Send", "RequestEditor", "ctrl-enter", "cmd-enter"),
    ("workspace::Save", "Workspace", "ctrl-s", "cmd-s"),
    ("workspace::NewTab", "Workspace", "ctrl-t", "cmd-t"),
    ("workspace::CloseTab", "Workspace", "ctrl-w", "cmd-w"),
    ("request::FocusUrlBar", "RequestEditor", "ctrl-l", "cmd-l"),
    (
        "workspace::ToggleConsole",
        "Workspace",
        "ctrl-alt-c",
        "cmd-alt-c",
    ),
    ("workspace::ToggleSidebar", "Workspace", "ctrl-\\", "cmd-\\"),
    (
        "palette::Toggle",
        "Workspace",
        "ctrl-shift-p",
        "cmd-shift-p",
    ),
    ("quick_open::Toggle", "Workspace", "ctrl-p", "cmd-p"),
    ("workspace::NextTab", "Workspace", "ctrl-tab", "cmd-tab"),
    (
        "workspace::PreviousTab",
        "Workspace",
        "ctrl-shift-tab",
        "cmd-shift-tab",
    ),
    ("request::Cancel", "RequestEditor", "escape", "escape"),
    ("response::Find", "ResponseViewer", "ctrl-f", "cmd-f"),
];

#[gpui::test]
fn every_keymap_entry_parses_on_all_platforms(cx: &mut TestAppContext) {
    for platform in KeymapPlatform::ALL {
        let entries = parse_keymap_entries(platform.json())
            .unwrap_or_else(|problem| panic!("{platform:?} keymap must parse: {problem}"));
        assert!(
            !entries.is_empty(),
            "{platform:?} keymap must contain bindings"
        );

        let problems = cx.update(|cx| load_keymap(cx, platform.json()));
        assert!(
            problems.is_empty(),
            "{platform:?} keymap entries must all bind: {problems:?}"
        );
    }
}

#[test]
fn prd_bindings_present() {
    for platform in KeymapPlatform::ALL {
        let entries = parse_keymap_entries(platform.json())
            .unwrap_or_else(|problem| panic!("{platform:?} keymap must parse: {problem}"));
        for &(action, context, ctrl_keystroke, cmd_keystroke) in PRD_KEYBINDINGS {
            let keystroke = match platform {
                KeymapPlatform::Macos => cmd_keystroke,
                KeymapPlatform::Linux | KeymapPlatform::Windows => ctrl_keystroke,
            };
            let present = entries.iter().any(|entry| {
                entry.action == action && entry.context == context && entry.keystroke == keystroke
            });
            assert!(
                present,
                "{platform:?} must bind {keystroke} to {action} in context {context}"
            );
        }
    }
}

#[test]
fn no_duplicate_bindings_in_same_context() {
    for platform in KeymapPlatform::ALL {
        let entries = parse_keymap_entries(platform.json())
            .unwrap_or_else(|problem| panic!("{platform:?} keymap must parse: {problem}"));
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for entry in &entries {
            let binding = (entry.context.as_str(), entry.keystroke.as_str());
            assert!(
                !seen.contains(&binding),
                "{platform:?} binds {} more than once in context {}",
                entry.keystroke,
                entry.context
            );
            seen.push(binding);
        }
    }
}
