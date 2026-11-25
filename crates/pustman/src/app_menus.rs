//! The app's one menu model (S48u2): File, Edit, View, Request and Help, each entry an
//! action that already exists, so the command palette lists it too, with the same keys.
//! [`init`] installs it with `workspace::install_app_menus` — macOS shows it in the
//! native menu bar, everywhere else the title bar draws it in the window — and installs
//! it again whenever the recent workspace list behind File › Open recent changes.
//!
//! `Decision:` the model is built here, in the app crate, because only the app depends on
//! every crate whose actions it lists (`collections::NewCollection`, the `text_input` edit
//! actions); `workspace` draws whatever model it is given and never names those crates.
//!
//! `Decision:` "New request" runs `workspace::NewTab`: the shell hands a new tab to the
//! request editor's `NewRequest` (`Workspace::new_tab`), and `NewTab` is the action the
//! keymap binds (Ctrl+T), so the entry shows the keys that do the same thing.
//!
//! `Decision:` Help › Keyboard shortcuts is the command palette — it lists every action
//! with its keys — so `palette::Toggle` is the one action with two entries (View ›
//! Command palette is the other). There is no About entry: nothing exists to show.

use gpui::{App, Menu, MenuItem, OsAction};
use pustman_actions::workspace as shell;
use pustman_actions::{environment, import_export, palette, quick_open, request};
use text_input::input as edit;
use workspace::recent_workspaces::{
    OpenRecentWorkspace, RecentEntry, RecentWorkspaces, disambiguation, recent_workspaces,
};

#[cfg(test)]
#[path = "app_menus_tests.rs"]
mod tests;

/// File › Open recent: the recent workspaces, newest first, and the picker over them.
pub const OPEN_RECENT_MENU: &str = "Open recent";

/// The model, with File › Open recent listing `recents` (newest first) under their own
/// names — the names the title bar's switcher shows — with the folder added only where
/// two share a name.
pub fn app_menus(recents: &[RecentEntry]) -> Vec<Menu> {
    let mut open_recent: Vec<MenuItem> = recents
        .iter()
        .enumerate()
        .map(|(index, recent)| {
            let label = match disambiguation(recents, index) {
                Some(path) => format!("{} — {path}", recent.name),
                None => recent.name.clone(),
            };
            let path = recent.path.to_string_lossy().into_owned();
            MenuItem::action(label, OpenRecentWorkspace { path })
        })
        .collect();
    if !open_recent.is_empty() {
        open_recent.push(MenuItem::separator());
    }
    open_recent.push(MenuItem::action("More…", shell::OpenRecent));

    let mut menus = Vec::new();
    if cfg!(target_os = "macos") {
        // macOS turns the first menu into the application menu, titled with the app's
        // name; it carries the system's Services so File stays File.
        menus.push(Menu::new("Pustman").items([MenuItem::os_submenu(
            "Services",
            gpui::SystemMenuType::Services,
        )]));
    }
    menus.extend([
        Menu::new("File").items([
            MenuItem::action("New request", shell::NewTab),
            MenuItem::action("New collection…", collections_panel::crud::NewCollection),
            MenuItem::separator(),
            MenuItem::action("New workspace…", shell::CreateWorkspace),
            MenuItem::action("Open workspace…", shell::OpenWorkspace),
            MenuItem::submenu(Menu::new(OPEN_RECENT_MENU).items(open_recent)),
            MenuItem::separator(),
            MenuItem::action("Import…", import_export::Import),
            MenuItem::action("Export…", import_export::Export),
            MenuItem::separator(),
            MenuItem::action("Save", shell::Save),
            MenuItem::action("Save all", shell::SaveAll),
            MenuItem::action("Close tab", shell::CloseTab),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", edit::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", edit::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", edit::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", edit::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", edit::Paste, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Select all", edit::SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle sidebar", shell::ToggleSidebar),
            MenuItem::action("Toggle console", shell::ToggleConsole),
            MenuItem::separator(),
            MenuItem::action("Command palette", palette::Toggle),
            MenuItem::action("Quick open", quick_open::Toggle),
        ]),
        Menu::new("Request").items([
            MenuItem::action("Send", request::Send),
            MenuItem::action("Cancel", request::Cancel),
            MenuItem::action("Focus URL bar", request::FocusUrlBar),
            MenuItem::separator(),
            MenuItem::action("Copy as cURL", request::CopyAsCurl),
            MenuItem::action("Paste cURL", request::PasteCurl),
            MenuItem::separator(),
            MenuItem::action("Environments…", environment::SelectEnvironment),
        ]),
        Menu::new("Help").items([MenuItem::action("Keyboard shortcuts", palette::Toggle)]),
    ]);
    menus
}

/// Install the model now and again whenever the recent workspace list changes.
pub fn init(cx: &mut App) {
    if cfg!(target_os = "macos") {
        workspace::use_native_menu_bar(cx);
    }
    install(cx);
    cx.observe_global::<RecentWorkspaces>(install).detach();
}

fn install(cx: &mut App) {
    let menus = app_menus(recent_workspaces(cx));
    workspace::install_app_menus(menus, cx);
}
