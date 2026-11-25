//! File › Open recent says so when the recent list could not be read (#332): a disabled
//! "could not be read" row instead of a submenu that looks empty.

use gpui::OwnedMenuItem;
use workspace::recent_workspaces::RECENT_READ_FAILED_LABEL;

use crate::app_menus::{OPEN_RECENT_MENU, app_menus_for};

/// The Open recent submenu's items, as `(label, disabled)`.
fn open_recent_items(read_failed: bool) -> Vec<(String, bool)> {
    let file = app_menus_for(&[], read_failed)
        .into_iter()
        .map(|menu| menu.owned())
        .find(|menu| menu.name.as_ref() == "File")
        .expect("the model has a File menu");
    file.items
        .into_iter()
        .find_map(|item| match item {
            OwnedMenuItem::Submenu(menu) if menu.name.as_ref() == OPEN_RECENT_MENU => {
                Some(menu.items)
            }
            _ => None,
        })
        .expect("File has Open recent")
        .into_iter()
        .filter_map(|item| match item {
            OwnedMenuItem::Action { name, disabled, .. } => Some((name.to_string(), disabled)),
            _ => None,
        })
        .collect()
}

#[test]
fn test_open_recent_shows_a_disabled_row_when_the_recent_read_failed() {
    let items = open_recent_items(true);
    assert!(
        items.contains(&(RECENT_READ_FAILED_LABEL.to_owned(), true)),
        "a failed read is a disabled row in Open recent, got {items:?}"
    );
}

#[test]
fn test_open_recent_has_no_failure_row_when_the_read_succeeded() {
    let items = open_recent_items(false);
    assert!(
        !items
            .iter()
            .any(|(name, _)| name == RECENT_READ_FAILED_LABEL),
        "no failure row after a good read, got {items:?}"
    );
}
