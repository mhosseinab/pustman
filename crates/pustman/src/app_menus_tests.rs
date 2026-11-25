//! The app's one menu model (S48u2): File, Edit, View, Request and Help, every entry an
//! action that already exists, each action listed once, and each entry showing the keys
//! the real default keymap binds its action to. File › Open recent is fed by the recent
//! workspace list.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::{Action, Menu, MenuItem, TestAppContext};
use ui::{keymap_binding_text, keystroke_label};

use crate::StoreLocation;
use crate::app_menus::{OPEN_RECENT_MENU, app_menus};

/// Every action entry in `menus` as (menu path, label, action), submenus included.
fn entries(menus: &[Menu]) -> Vec<(String, String, Box<dyn Action>)> {
    fn walk(path: &str, items: &[MenuItem], out: &mut Vec<(String, String, Box<dyn Action>)>) {
        for item in items {
            match item {
                MenuItem::Action { name, action, .. } => {
                    out.push((path.to_owned(), name.to_string(), action.boxed_clone()));
                }
                MenuItem::Submenu(submenu) => {
                    walk(&format!("{path} › {}", submenu.name), &submenu.items, out);
                }
                MenuItem::Separator | MenuItem::SystemMenu(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus {
        walk(&menu.name, &menu.items, &mut out);
    }
    out
}

#[test]
fn test_the_menu_bar_is_file_edit_view_request_help() {
    let names: Vec<String> = app_menus(&[])
        .iter()
        .map(|menu| menu.name.to_string())
        .filter(|name| name != "Pustman")
        .collect();
    assert_eq!(names, ["File", "Edit", "View", "Request", "Help"]);
}

#[test]
fn test_each_action_is_listed_once_except_keyboard_shortcuts_which_is_the_palette() {
    let menus = app_menus(&[]);
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, label, action) in entries(&menus) {
        seen.entry(action.name().to_owned())
            .or_default()
            .push(format!("{path} › {label}"));
    }
    for (action, places) in &seen {
        let allowed = if action == "palette::Toggle" { 2 } else { 1 };
        assert_eq!(
            places.len(),
            allowed,
            "{action} is listed {places:?}; one entry per action (Help › Keyboard shortcuts \
             is the command palette's second door)"
        );
    }
    for required in [
        "workspace::NewTab",
        "collections::NewCollection",
        "workspace::CreateWorkspace",
        "workspace::OpenWorkspace",
        "workspace::OpenRecent",
        "import_export::Import",
        "import_export::Export",
        "workspace::Save",
        "workspace::SaveAll",
        "workspace::CloseTab",
        "text_input::Undo",
        "text_input::Redo",
        "text_input::Cut",
        "text_input::Copy",
        "text_input::Paste",
        "text_input::SelectAll",
        "workspace::ToggleSidebar",
        "workspace::ToggleConsole",
        "palette::Toggle",
        "quick_open::Toggle",
        "request::Send",
        "request::Cancel",
        "request::FocusUrlBar",
        "request::CopyAsCurl",
        "request::PasteCurl",
        "environment::SelectEnvironment",
    ] {
        assert!(seen.contains_key(required), "the menus list {required}");
    }
}

#[test]
fn test_open_recent_lists_the_recent_workspaces_newest_first() {
    let recents = [PathBuf::from("/work/orders"), PathBuf::from("/work/pets")];
    let menus = app_menus(&recents);
    let open_recent: Vec<(String, String)> = entries(&menus)
        .into_iter()
        .filter(|(path, _, _)| path.ends_with(OPEN_RECENT_MENU))
        .map(|(_, label, action)| (label, action.name().to_owned()))
        .collect();
    assert_eq!(
        open_recent,
        [
            (
                recents[0].to_string_lossy().into_owned(),
                "workspace::OpenRecentWorkspace".to_owned()
            ),
            (
                recents[1].to_string_lossy().into_owned(),
                "workspace::OpenRecentWorkspace".to_owned()
            ),
            ("More…".to_owned(), "workspace::OpenRecent".to_owned()),
        ]
    );
}

#[gpui::test]
fn test_entries_show_the_bindings_of_the_real_default_keymap(cx: &mut TestAppContext) {
    cx.update(|cx| crate::init_with_store(cx, StoreLocation::InMemory));
    cx.run_until_parked();
    let menus = app_menus(&[]);
    let hints: BTreeMap<String, Option<String>> = cx.update(|cx| {
        entries(&menus)
            .into_iter()
            .map(|(_, label, action)| {
                let keys = keymap_binding_text(action.as_ref(), cx).map(|keys| keys.to_string());
                (label, keys)
            })
            .collect()
    });
    let os = cx.update(|cx| cx.platform_info().os);
    // The keys as the keymap file spells them, through the one label function hints use.
    let key = |linux: &str, macos: &str| {
        let source = if os == gpui::OperatingSystem::Mac {
            macos
        } else {
            linux
        };
        let keystroke = gpui::Keystroke::parse_with_os(source, os).expect("the test key parses");
        Some(keystroke_label(&keystroke.modifiers, &keystroke.key, os))
    };
    assert_eq!(hints["Save"], key("ctrl-s", "cmd-s"));
    assert_eq!(hints["Close tab"], key("ctrl-w", "cmd-w"));
    assert_eq!(hints["New request"], key("ctrl-t", "cmd-t"));
    assert_eq!(hints["Send"], key("ctrl-enter", "cmd-enter"));
    assert_eq!(hints["Quick open"], key("ctrl-p", "cmd-p"));
    assert_eq!(hints["Copy"], key("ctrl-c", "cmd-c"));
    assert!(
        hints["Copy as cURL"].is_none(),
        "an action the keymap leaves unbound shows no keys"
    );
}

#[gpui::test]
fn test_the_installed_model_is_what_the_platform_menu_holds(cx: &mut TestAppContext) {
    cx.update(|cx| crate::init_with_store(cx, StoreLocation::InMemory));
    cx.run_until_parked();
    let installed: Vec<String> = cx.update(|cx| {
        cx.get_menus()
            .unwrap_or_default()
            .into_iter()
            .map(|menu| menu.name.to_string())
            .collect()
    });
    let expected: Vec<String> = app_menus(&[])
        .iter()
        .map(|menu| menu.name.to_string())
        .collect();
    assert_eq!(installed, expected, "the init order installs the model");
}
