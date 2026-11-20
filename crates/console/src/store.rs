//! The [`ConsoleStore`]: the Console's entries in a capped, in-memory ring buffer.
//!
//! The buffer is a [`VecDeque`] with a ceiling — the `console_max_entries` setting
//! (REQ-CON-02) — and the entry that has been in it longest is the one dropped when a
//! push runs out of room. Pausing drops what arrives instead of recording it, so a busy
//! feed can be stilled without losing the entries already on screen. Nothing in this
//! module writes to disk or logs content: the buffer lives exactly as long as the app
//! does (REQ-CON-06, REQ-NFR-04).

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use api_model::Method;
use gpui::{App, Global, Subscription};
use settings::Setting;

/// One Console row (REQ-CON-01): the summary line the list shows — method, URL, status,
/// time and size — plus the level, the protocol and the raw text the expanded row shows
/// (REQ-CON-03).
///
/// The raw text is stored exactly as the producer hands it over: redaction happens at
/// the producer (the send's secrets are only known there), and this type's [`Debug`]
/// keeps it out of every log.
#[derive(Clone, PartialEq, Eq)]
pub struct ConsoleEntry {
    /// What the entry flags: an ordinary record, a warning or an error (REQ-CON-02
    /// filters by it).
    pub level: ConsoleLevel,
    /// The protocol that produced the entry, named as `storage`'s history rows name it
    /// (`http` for every v1 send — plan D7 models no other).
    pub protocol: String,
    /// The request method. `None` when the entry names no method: the summary line shows
    /// method *or* protocol (REQ-CON-01), and a warning can outlive no request of its own.
    pub method: Option<Method>,
    /// The URL the entry is about.
    pub url: String,
    /// The response status code. `None` when no response arrived — warnings and
    /// cancelled sends (REQ-HTTP-12) are entries too (REQ-CON-04).
    pub status: Option<u16>,
    /// How long the exchange took. `None` when it never completed.
    pub time: Option<Duration>,
    /// The response size in bytes. `None` when there is no response.
    pub size: Option<u64>,
    /// The request as it went on the wire (REQ-CON-03).
    pub raw_request: String,
    /// The response or events as they arrived (REQ-CON-03).
    pub raw_response: String,
}

impl fmt::Debug for ConsoleEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The raw text is request and response content — bodies, Authorization headers
        // and cookies travel in it (AGENTS.md › Security) — so its size is printed and
        // its text is not, the way `http_engine`'s `ResponseBody` prints a body.
        f.debug_struct("ConsoleEntry")
            .field("level", &self.level)
            .field("protocol", &self.protocol)
            .field("method", &self.method)
            .field("url", &self.url)
            .field("status", &self.status)
            .field("time", &self.time)
            .field("size", &self.size)
            .field("raw_request_bytes", &self.raw_request.len())
            .field("raw_response_bytes", &self.raw_response.len())
            .finish()
    }
}

/// What an entry flags (REQ-CON-02: filter by level "info, warning, error").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConsoleLevel {
    /// An ordinary record: a completed send or session.
    Info,
    /// Something the user should know about — unresolved variables, TLS verification
    /// disabled, a missing body file (REQ-CON-04).
    Warning,
    /// A send or session that failed.
    Error,
}

/// The `console` slice of the settings: how many entries the ring buffer holds
/// (REQ-CON-02, `console_max_entries`).
pub struct ConsoleSetting {
    /// The cap the ring buffer keeps to. The setting is a `u32` a user can write, so the
    /// conversion here is checked rather than assumed.
    pub max_entries: usize,
}

impl Setting for ConsoleSetting {
    fn from_settings(settings: &settings::Settings) -> Self {
        Self {
            max_entries: usize::try_from(settings.console_max_entries).unwrap_or(usize::MAX),
        }
    }
}

/// The Console's entries: a ring buffer of [`ConsoleEntry`] capped by the
/// `console_max_entries` setting, with a pause flag that drops new arrivals.
///
/// Reads go through [`Self::entries`] after `cx.global::<ConsoleStore>()`; pushes and
/// other mutations go through the functions taking `cx`, which is also what tells the
/// views observing this global to redraw.
pub struct ConsoleStore {
    entries: VecDeque<ConsoleEntry>,
    max_entries: usize,
    paused: bool,
    /// Held so the cap keeps following the `console_max_entries` setting. Dropping it —
    /// a second [`crate::init`], or the app quitting — is what stops the old store
    /// listening.
    _settings_subscription: Subscription,
}

impl Global for ConsoleStore {}

impl ConsoleStore {
    /// Record one entry. While the store is paused the entry is dropped — that is what
    /// pausing means (REQ-CON-02) — and when the buffer is at the cap the entry that has
    /// been in it longest is dropped first (REQ-CON-02).
    pub fn push(cx: &mut App, entry: ConsoleEntry) {
        let store = cx.global_mut::<ConsoleStore>();
        if store.paused {
            return;
        }
        store.entries.push_back(entry);
        store.trim();
    }

    /// Empty the buffer (REQ-CON-02).
    pub fn clear(cx: &mut App) {
        cx.global_mut::<ConsoleStore>().entries.clear();
    }

    /// Pause or resume recording: paused, [`Self::push`] drops what arrives (REQ-CON-02).
    pub fn set_paused(cx: &mut App, paused: bool) {
        cx.global_mut::<ConsoleStore>().paused = paused;
    }

    /// The entries in the buffer, oldest first: iterate them, or index for the row a
    /// list draws. A [`VecDeque`] is handed out rather than one slice because a ring
    /// that drops from the front wraps and holds two slices at once.
    pub fn entries(&self) -> &VecDeque<ConsoleEntry> {
        &self.entries
    }

    /// How many entries the buffer holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the buffer holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether new entries are being dropped.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// A store capped at `max_entries`, following `settings_subscription`.
    pub(crate) fn new(max_entries: usize, settings_subscription: Subscription) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
            paused: false,
            _settings_subscription: settings_subscription,
        }
    }

    /// Follow the `console_max_entries` setting to a new cap. A smaller cap trims the
    /// buffer right away — the oldest entries go first, exactly as on a push over the
    /// cap — so a user who shrinks the setting sees the memory come back now.
    pub(crate) fn set_max_entries(&mut self, max_entries: usize) {
        self.max_entries = max_entries;
        self.trim();
    }

    /// Drop the oldest entries until the buffer fits the cap again.
    fn trim(&mut self) {
        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
