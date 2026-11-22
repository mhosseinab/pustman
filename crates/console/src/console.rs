//! `console` — the Console's entries, in memory only (PRD §8.3).
//!
//! One [`ConsoleEntry`] lands per send and per warning or cancel (REQ-CON-01,
//! REQ-CON-04): a summary line — method, URL, status, time and size — plus the level,
//! the protocol and the raw request and response text exactly as they went on the wire
//! (REQ-CON-03). [`ConsoleStore`] holds them in a ring buffer capped by the
//! `console_max_entries` setting and drops the oldest first when it runs out of room
//! (REQ-CON-02). Nothing here is ever written to disk unless the user exports it
//! (REQ-CON-06), and nothing here is logged: [`ConsoleEntry`]'s `Debug` prints the raw
//! text's size, never its content, which can hold Authorization headers and cookies
//! (REQ-NFR-04).
//!
//! [`init`] installs the store; producers push through [`ConsoleStore::push`] and the
//! Console panel reads through [`ConsoleStore::entries`] (S32d).

mod export;
mod feed;
mod panel;
mod store;

pub use export::export_text;
pub use feed::{record_error, record_outcome, record_warnings, watch};
pub use panel::ConsolePanel;
pub use store::{ConsoleEntry, ConsoleLevel, ConsoleSetting, ConsoleStore};

use gpui::{App, AppContext};
use settings::Setting;
use workspace::Workspace;

/// Install the [`ConsoleStore`] global together with its slice of the settings.
///
/// Called once at startup, after `settings`, in the init order `main.rs` runs. The cap
/// follows the `console_max_entries` setting from then on: a change trims the buffer to
/// the new size, dropping the oldest entries first.
pub fn init(cx: &mut App) {
    ConsoleSetting::register(cx);
    let max_entries = ConsoleSetting::get_global(cx).max_entries;
    let settings_subscription = cx.observe_global::<settings::SettingsStore>(|cx| {
        let max_entries = ConsoleSetting::get_global(cx).max_entries;
        cx.global_mut::<ConsoleStore>().set_max_entries(max_entries);
    });
    cx.set_global(ConsoleStore::new(max_entries, settings_subscription));
    // Every workspace gets a [`ConsolePanel`] in its bottom dock (S32d). Detached: the
    // app keeps exactly one of these observers for as long as it runs. Registration is
    // skipped when the workspace is created outside a window update — without a window
    // the panel's text field cannot even be built, and `Workspace::new` runs inside one.
    cx.observe_new::<Workspace>(|workspace, window, cx| {
        let Some(window) = window else {
            return;
        };
        let panel = cx.new(|cx| ConsolePanel::new(window, cx));
        workspace.register_panel(panel, window, cx);
    })
    .detach();
}
