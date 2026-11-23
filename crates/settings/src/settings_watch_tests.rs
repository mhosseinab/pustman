//! Coverage for the [`SettingsStore::watch`] loop (REQ-UI-08): the [`RELOAD_INTERVAL`]
//! poll that reads the user settings file off the foreground thread and decides what — if
//! anything — to merge. Every test drives the loop through the GPUI clock, never real
//! time, and points it at a temp dir instead of the user's real settings file.

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{Subscription, TestAppContext};
use tempfile::TempDir;

use crate::{RELOAD_INTERVAL, Setting, Settings, SettingsError, SettingsStore};

/// The settings file the loop polls inside `dir`.
fn user_file(dir: &TempDir) -> PathBuf {
    dir.path().join("settings.json")
}

/// One poll of the watch loop: advance past [`RELOAD_INTERVAL`] and drain the tasks.
fn poll(cx: &mut TestAppContext) {
    cx.executor().advance_clock(RELOAD_INTERVAL);
    cx.run_until_parked();
}

/// A counter of [`SettingsStore`] notifications, plus the subscription that keeps it
/// live — a dropped subscription stops firing.
fn notification_counter(cx: &mut TestAppContext) -> (Rc<Cell<usize>>, Subscription) {
    let counter = Rc::new(Cell::new(0));
    let subscription = cx.update(|cx| {
        let counter = Rc::clone(&counter);
        cx.observe_global::<SettingsStore>(move |_cx| {
            counter.set(counter.get() + 1);
        })
    });
    // `observe_global` activates on a deferred effect: let it land before the next poll.
    cx.run_until_parked();
    (counter, subscription)
}

#[gpui::test]
fn watch_applies_changed_user_text(cx: &mut TestAppContext) {
    // A save that changes the file's text is merged over the defaults and lands in the
    // typed views the rest of the app reads.
    let dir = tempfile::tempdir().expect("temp settings dir");
    let path = user_file(&dir);
    cx.update(|cx| {
        SettingsStore::init(cx, None).expect("the defaults merge");
        Settings::register(cx);
        SettingsStore::watch_file(cx, path.clone());
    });

    fs::write(&path, r#"{"ui_font_size": 20}"#).expect("write the user file");
    poll(cx);

    cx.update(|cx| {
        assert_eq!(
            Settings::get_global(cx).ui_font_size,
            20,
            "the changed text is merged over the defaults"
        );
    });
}

#[gpui::test]
fn watch_ignores_unchanged_user_text(cx: &mut TestAppContext) {
    // A poll that reads text the store was already built from applies nothing and
    // notifies nobody, so an observing UI does not re-run on every tick.
    let dir = tempfile::tempdir().expect("temp settings dir");
    let path = user_file(&dir);
    cx.update(|cx| {
        SettingsStore::init(cx, None).expect("the defaults merge");
        Settings::register(cx);
        SettingsStore::watch_file(cx, path.clone());
    });
    let (notifications, _subscription) = notification_counter(cx);

    fs::write(&path, r#"{"ui_font_size": 20}"#).expect("write the user file");
    poll(cx);
    assert_eq!(notifications.get(), 1, "a changed file notifies once");

    poll(cx);
    poll(cx);
    assert_eq!(
        notifications.get(),
        1,
        "re-reading the same text is a no-op"
    );
    cx.update(|cx| assert_eq!(Settings::get_global(cx).ui_font_size, 20));
}

#[gpui::test]
fn watch_missing_file_leaves_the_store_alone(cx: &mut TestAppContext) {
    // A missing file reads as `Ok(None)`, not as an error: nothing is merged and nothing
    // is recorded, because a poll cannot tell a delete from an atomic save mid-swap.
    let dir = tempfile::tempdir().expect("temp settings dir");
    let path = user_file(&dir);
    cx.update(|cx| {
        SettingsStore::init(cx, None).expect("the defaults merge");
        Settings::register(cx);
        SettingsStore::watch_file(cx, path.clone());
    });
    let (notifications, _subscription) = notification_counter(cx);

    poll(cx);
    poll(cx);

    assert_eq!(
        notifications.get(),
        0,
        "nothing changed, so nobody was notified"
    );
    cx.update(|cx| {
        assert_eq!(
            SettingsStore::last_error(cx),
            None,
            "a missing file is not an error"
        );
        assert_eq!(
            Settings::get_global(cx).ui_font_size,
            16,
            "the defaults stand"
        );
    });
}

#[gpui::test]
fn watch_records_a_read_error_and_keeps_last_good_values(cx: &mut TestAppContext) {
    // A settings path that cannot be read (here a directory stands in for the file) is
    // recorded for the UI to show, while the values the store was built on stand.
    let dir = tempfile::tempdir().expect("temp settings dir");
    let path = user_file(&dir);
    fs::create_dir(&path).expect("an unreadable stand-in for the user file");
    cx.update(|cx| {
        SettingsStore::init(cx, Some(r#"{"ui_font_size": 20}"#)).expect("the good blob merges");
        Settings::register(cx);
        SettingsStore::watch_file(cx, path.clone());
    });

    poll(cx);

    cx.update(|cx| {
        assert!(
            matches!(SettingsStore::last_error(cx), Some(SettingsError::Read(_))),
            "a file that cannot be read lands on last_error"
        );
        assert_eq!(
            Settings::get_global(cx).ui_font_size,
            20,
            "the last good values stand"
        );
    });
}

#[gpui::test]
fn unchanged_read_error_notifies_observers_once(cx: &mut TestAppContext) {
    // A file that stays unreadable is re-read every poll, but the error does not change:
    // only the first failure may notify, or an observing UI re-runs (and would re-toast)
    // on every tick although `last_error` is unchanged.
    let dir = tempfile::tempdir().expect("temp settings dir");
    let path = user_file(&dir);
    fs::create_dir(&path).expect("an unreadable stand-in for the user file");
    cx.update(|cx| {
        SettingsStore::init(cx, None).expect("the defaults merge");
        Settings::register(cx);
        SettingsStore::watch_file(cx, path.clone());
    });
    let (notifications, _subscription) = notification_counter(cx);

    poll(cx);
    assert_eq!(
        notifications.get(),
        1,
        "the first failure is recorded and notifies once"
    );

    poll(cx);
    poll(cx);
    assert_eq!(
        notifications.get(),
        1,
        "the same failure does not notify again"
    );
}

#[gpui::test]
fn second_watch_replaces_the_running_watcher(cx: &mut TestAppContext) {
    // A second watch drops the running watcher: the replaced loop stops applying its
    // file and only the newest one is polled.
    let first_dir = tempfile::tempdir().expect("temp settings dir");
    let first_path = user_file(&first_dir);
    let second_dir = tempfile::tempdir().expect("temp settings dir");
    let second_path = user_file(&second_dir);
    cx.update(|cx| {
        SettingsStore::init(cx, None).expect("the defaults merge");
        Settings::register(cx);
        SettingsStore::watch_file(cx, first_path.clone());
    });

    fs::write(&first_path, r#"{"ui_font_size": 20}"#).expect("write the first file");
    poll(cx);
    cx.update(|cx| assert_eq!(Settings::get_global(cx).ui_font_size, 20));

    cx.update(|cx| SettingsStore::watch_file(cx, second_path.clone()));

    fs::write(&first_path, r#"{"ui_font_size": 30}"#).expect("write the first file again");
    poll(cx);
    cx.update(|cx| {
        assert_eq!(
            Settings::get_global(cx).ui_font_size,
            20,
            "the replaced watcher no longer applies its file"
        );
    });

    fs::write(&second_path, r#"{"ui_font_size": 40}"#).expect("write the second file");
    poll(cx);
    cx.update(|cx| {
        assert_eq!(
            Settings::get_global(cx).ui_font_size,
            40,
            "the newest watcher applies its file"
        );
    });
}
