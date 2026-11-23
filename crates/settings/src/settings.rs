//! `settings` — typed app settings: bundled defaults overlaid with a user file.
//!
//! [`load_defaults`] parses the JSON embedded from `assets/settings/default.json` — the
//! same file users read and edit, so what ships is what is documented. [`merge`] lays a
//! user's `settings.json` over those defaults key by key: a key the user sets wins, a key
//! they leave out keeps the default, and keys this crate does not know are ignored. A
//! blob that does not parse answers [`SettingsError::Parse`] and changes nothing, so a
//! broken user file can never destroy the settings it was built on.
//!
//! The GPUI side lives here too: [`SettingsStore`] is the global that holds the merged
//! [`Settings`], and [`Setting`] is the trait a feature crate implements to read its own
//! slice of them. [`paths`] names the user settings file and [`SettingsStore::watch`]
//! keeps the store in step with it, re-merging its text over the defaults whenever that
//! text changes. A blob that does not parse leaves the last good values standing; its
//! error is returned to the caller and recorded on [`SettingsStore::last_error`], so a
//! caller and the background watcher both get to show it.

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{App, AppContext, Global, Task};
use serde::Deserialize;

/// The bundled defaults, embedded at compile time.
const DEFAULTS_JSON: &str = include_str!("../../../assets/settings/default.json");

/// Which appearance the app draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Always draw the light theme.
    Light,
    /// Always draw the dark theme.
    Dark,
    /// Follow the operating system's appearance.
    System,
}

/// Every setting resolved to one concrete value.
///
/// Field names are the keys of `assets/settings/default.json`; [`Settings::request`] is
/// the nested object behind the `request.*` keys.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Settings {
    /// Follow the OS appearance or force one (REQ-UI-08).
    pub theme: Theme,
    /// Point size of the app's own text.
    pub ui_font_size: u32,
    /// Point size of text in request and response editors.
    pub buffer_font_size: u32,
    /// Defaults every request sends with (REQ-HTTP-09).
    pub request: RequestSettings,
    /// Days of history kept before old entries are dropped (REQ-HIST-04).
    pub history_retention_days: u32,
    /// Console entries the ring buffer holds (REQ-CON-02).
    pub console_max_entries: u32,
}

/// Global request defaults (`request.*` keys). A request overrides one per key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct RequestSettings {
    /// Milliseconds a request may run before it is given up on.
    pub timeout_ms: u64,
    /// Follow `3xx` responses to the next URL.
    pub follow_redirects: bool,
    /// Redirects followed before the request is given up on.
    pub max_redirects: u32,
    /// Verify the server's TLS certificate. Turning this off is per-request and flagged.
    pub verify_tls: bool,
    /// Megabytes of response body that are pretty-printed; larger bodies show raw.
    pub response_pretty_limit_mb: u64,
}

/// Why the settings could not be updated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// The text is not a settings object this crate understands. The message is the
    /// parser's own, kept for the user who has to fix the file.
    #[error("could not parse settings: {0}")]
    Parse(String),
    /// The settings file could not be read. The message is the operating system's own,
    /// kept for the user who has to fix it; the store keeps the last good values.
    #[error("could not read settings: {0}")]
    Read(String),
}

impl Default for Settings {
    /// The documented defaults — the values `assets/settings/default.json` carries and
    /// [`load_defaults`] hands back. Kept in code because [`load_defaults`] cannot panic
    /// and must still answer with settings when the embedded JSON somehow does not
    /// parse; `defaults_load` pins both copies to the same numbers.
    fn default() -> Self {
        Self {
            theme: Theme::System,
            ui_font_size: 16,
            buffer_font_size: 14,
            request: RequestSettings {
                timeout_ms: 30_000,
                follow_redirects: true,
                max_redirects: 10,
                verify_tls: true,
                response_pretty_limit_mb: 10,
            },
            history_retention_days: 30,
            console_max_entries: 5000,
        }
    }
}

/// The bundled defaults as typed settings.
pub fn load_defaults() -> Settings {
    serde_json::from_str(DEFAULTS_JSON).unwrap_or_default()
}

/// Overlay a user file onto `base`, key by key.
///
/// `user_json` is the text of a user's `settings.json`, or `None` when there is no user
/// file yet. Keys the user sets replace the same key in `base`, keys the user leaves out
/// are kept as they are, and unknown keys are ignored. A nested object such as `request`
/// is overlaid key by key too, so setting one of its keys leaves the others alone.
///
/// # Errors
///
/// [`SettingsError::Parse`] when `user_json` is not a settings object this crate can
/// read. Nothing is applied in that case: the caller keeps the settings it already had.
pub fn merge(base: Settings, user_json: Option<&str>) -> Result<Settings, SettingsError> {
    let Some(user_json) = user_json else {
        return Ok(base);
    };
    let overlay = serde_json::from_str::<Overlay>(user_json)
        .map_err(|error| SettingsError::Parse(error.to_string()))?;
    Ok(overlay.apply(base))
}

/// A user file as read: only the keys the user set. An absent key is `None`, and serde
/// drops keys it does not know, which is how unknown keys stay harmless.
#[derive(Debug, Default, Deserialize)]
struct Overlay {
    theme: Option<Theme>,
    ui_font_size: Option<u32>,
    buffer_font_size: Option<u32>,
    request: Option<RequestOverlay>,
    history_retention_days: Option<u32>,
    console_max_entries: Option<u32>,
}

/// The `request` object of a user file, read the same way as [`Overlay`].
#[derive(Debug, Default, Deserialize)]
struct RequestOverlay {
    timeout_ms: Option<u64>,
    follow_redirects: Option<bool>,
    max_redirects: Option<u32>,
    verify_tls: Option<bool>,
    response_pretty_limit_mb: Option<u64>,
}

impl Overlay {
    fn apply(self, mut base: Settings) -> Settings {
        if let Some(theme) = self.theme {
            base.theme = theme;
        }
        if let Some(ui_font_size) = self.ui_font_size {
            base.ui_font_size = ui_font_size;
        }
        if let Some(buffer_font_size) = self.buffer_font_size {
            base.buffer_font_size = buffer_font_size;
        }
        if let Some(request) = self.request {
            base.request = request.apply(base.request);
        }
        if let Some(history_retention_days) = self.history_retention_days {
            base.history_retention_days = history_retention_days;
        }
        if let Some(console_max_entries) = self.console_max_entries {
            base.console_max_entries = console_max_entries;
        }
        base
    }
}

impl RequestOverlay {
    fn apply(self, mut base: RequestSettings) -> RequestSettings {
        if let Some(timeout_ms) = self.timeout_ms {
            base.timeout_ms = timeout_ms;
        }
        if let Some(follow_redirects) = self.follow_redirects {
            base.follow_redirects = follow_redirects;
        }
        if let Some(max_redirects) = self.max_redirects {
            base.max_redirects = max_redirects;
        }
        if let Some(verify_tls) = self.verify_tls {
            base.verify_tls = verify_tls;
        }
        if let Some(response_pretty_limit_mb) = self.response_pretty_limit_mb {
            base.response_pretty_limit_mb = response_pretty_limit_mb;
        }
        base
    }
}

/// Where the user settings file lives.
pub mod paths {
    use std::path::PathBuf;

    /// The directory the user settings file lives in: the platform config directory with
    /// `pustman` under it — `~/.config/pustman` on Linux,
    /// `~/Library/Application Support/pustman` on macOS and `%APPDATA%\pustman` on
    /// Windows. A session that sets none of the environment those come from gets a
    /// relative directory, and with it no user settings file to read.
    pub fn config_dir() -> PathBuf {
        #[cfg(target_os = "macos")]
        let platform = home().join("Library").join("Application Support");
        #[cfg(target_os = "windows")]
        let platform = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData").join("Roaming"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let platform = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"));
        platform.join("pustman")
    }

    /// The user settings file itself: [`config_dir`]`/settings.json`.
    pub fn user_settings_file() -> PathBuf {
        config_dir().join("settings.json")
    }

    /// The user's home directory, or an empty path when the session sets neither `HOME`
    /// nor `USERPROFILE`.
    fn home() -> PathBuf {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_default()
    }
}

/// The user settings file's text, for [`SettingsStore::watch`] to read off the foreground
/// thread: `None` when the file is not there, and [`SettingsError::Read`] when it could
/// not be read.
fn read_user_file(path: &Path) -> Result<Option<String>, SettingsError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SettingsError::Read(error.to_string())),
    }
}

/// How often [`SettingsStore::watch`] looks at the user settings file.
const RELOAD_INTERVAL: Duration = Duration::from_millis(500);

/// The merged [`Settings`] as a GPUI global, together with the typed views registered
/// from it.
///
/// The app installs it once at startup with [`SettingsStore::init`]; everything else
/// reads through a [`Setting`] implementation of its own, so nothing outside this crate
/// reaches into the merged [`Settings`] directly.
pub struct SettingsStore {
    settings: Settings,
    registered: HashMap<TypeId, RegisteredView>,
    /// The error of the most recent merge that could not be applied, for the UI to show.
    /// `None` once a merge succeeds again.
    last_error: Option<SettingsError>,
    /// The task [`SettingsStore::watch`] runs. Dropping it — a second `watch`, or the app
    /// quitting — stops the watching.
    watch_task: Option<Task<()>>,
}

/// One registered type's current view of the settings, plus the function that rebuilds
/// that view when the merged settings change.
struct RegisteredView {
    build: fn(&Settings) -> Box<dyn Any>,
    value: Box<dyn Any>,
}

impl Global for SettingsStore {}

impl Default for SettingsStore {
    /// [`load_defaults`] with nothing registered yet: what a user without a `settings.json`
    /// gets, and what [`Setting::register`] builds on when it runs before [`SettingsStore::init`].
    fn default() -> Self {
        Self {
            settings: load_defaults(),
            registered: HashMap::new(),
            last_error: None,
            watch_task: None,
        }
    }
}

impl SettingsStore {
    /// Install the store as the app's global settings: [`load_defaults`] overlaid with
    /// `user_json`, the text of a user's `settings.json` when there is one.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Parse`] when `user_json` is not a settings object this crate can
    /// read. Whatever the store already holds is left alone, so a broken user file can
    /// never destroy the settings it was built on.
    pub fn init(cx: &mut App, user_json: Option<&str>) -> Result<(), SettingsError> {
        Self::reload(cx, user_json)
    }

    /// Re-merge `user_json` over the bundled defaults and update the store: what
    /// [`Self::watch`] does when the user file changes, and what a caller holding a fresh
    /// blob uses directly. Observers registered with [`App::observe_global`] see the new
    /// values.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Parse`] when `user_json` is not a settings object this crate can
    /// read. The store keeps the last good values and records the error on
    /// [`Self::last_error`] as well, so this caller and the background watcher both get
    /// to show it.
    pub fn reload(cx: &mut App, user_json: Option<&str>) -> Result<(), SettingsError> {
        Self::apply(cx, user_json);
        cx.global::<SettingsStore>().applied_result()
    }

    /// The error the most recent [`Self::reload`] left behind, for the UI to show. `None`
    /// when the last merge succeeded, none has run yet, or the store is not installed.
    pub fn last_error(cx: &App) -> Option<SettingsError> {
        cx.try_global::<SettingsStore>()
            .and_then(|store| store.last_error.clone())
    }

    /// Keep the store in step with the user settings file ([`paths::user_settings_file`]):
    /// a background task re-reads the file every [`RELOAD_INTERVAL`] and re-merges its
    /// text over the defaults whenever that text changes, notifying observers.
    ///
    /// The file is read off the foreground thread. A file that is missing leaves the
    /// store alone — a poll cannot tell a delete from the moment an editor swaps its
    /// atomic save into place, and this store is never cleared for one. A file that
    /// cannot be read keeps the last good values and lands on [`Self::last_error`].
    /// Calling `watch` again replaces the running watcher.
    pub fn watch(cx: &mut App) {
        Self::watch_file(cx, paths::user_settings_file());
    }

    /// The [`Self::watch`] loop over `path` instead of [`paths::user_settings_file`], so
    /// the loop itself is testable against a temp dir rather than the user's real settings
    /// file. Calling it again replaces the running watcher of either entry point.
    fn watch_file(cx: &mut App, path: PathBuf) {
        let task = cx.spawn(async move |cx| {
            let mut last_seen: Option<String> = None;
            loop {
                cx.background_executor().timer(RELOAD_INTERVAL).await;
                let read = cx
                    .background_spawn({
                        let path = path.clone();
                        async move { read_user_file(&path) }
                    })
                    .await;
                match read {
                    Ok(Some(text)) => {
                        if last_seen.as_deref() == Some(text.as_str()) {
                            continue;
                        }
                        last_seen = Some(text.clone());
                        cx.update(|cx| Self::apply(cx, Some(&text)));
                    }
                    // No file: nothing to merge. `last_seen` keeps the text the store was
                    // last built from, so a later save still counts as a change.
                    Ok(None) => {}
                    Err(error) => cx.update(|cx| Self::record_error(cx, error)),
                }
            }
        });
        cx.default_global::<SettingsStore>().watch_task = Some(task);
    }

    /// Merge `user_json` over the bundled defaults and install the result: the one place
    /// a merge is decided, so [`Self::init`], [`Self::reload`] and [`Self::watch`] share
    /// one error recovery. A blob that does not parse keeps the last good values and is
    /// recorded on [`Self::last_error`]; observers are notified either way.
    fn apply(cx: &mut App, user_json: Option<&str>) {
        let store = cx.default_global::<SettingsStore>();
        match merge(load_defaults(), user_json) {
            Ok(settings) => {
                store.last_error = None;
                store.set_settings(settings);
            }
            Err(error) => store.last_error = Some(error),
        }
    }

    /// Record a failure that came with no blob to merge ([`Self::watch`] finding a file
    /// it cannot read): the store keeps the last good values and the error lands on
    /// [`Self::last_error`] for the UI to show. An error that is already recorded is left
    /// alone, so a poll that reads the same failure again does not notify observers —
    /// nothing an observer could act on has changed.
    fn record_error(cx: &mut App, error: SettingsError) {
        let already_recorded = cx
            .try_global::<SettingsStore>()
            .is_some_and(|store| store.last_error.as_ref() == Some(&error));
        if already_recorded {
            return;
        }
        cx.default_global::<SettingsStore>().last_error = Some(error);
    }

    /// The outcome of the merge [`Self::apply`] just ran, for callers that report it.
    fn applied_result(&self) -> Result<(), SettingsError> {
        match &self.last_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// Replace the merged settings and rebuild every registered view from them, so a
    /// read after a change sees the change however registration was ordered against it.
    fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
        for view in self.registered.values_mut() {
            view.value = (view.build)(&self.settings);
        }
    }

    /// Record `T`'s view of the current settings so [`Setting::get_global`] can hand it out.
    fn register<T: Setting>(&mut self) {
        self.registered.insert(
            TypeId::of::<T>(),
            RegisteredView {
                build: |settings: &Settings| -> Box<dyn Any> {
                    Box::new(T::from_settings(settings))
                },
                value: Box::new(T::from_settings(&self.settings)),
            },
        );
    }

    /// The registered view of type `T`. Reading a type nobody registered is a programming
    /// error rather than something a user can cause, so it panics here instead of
    /// inventing a value the caller did not ask for.
    fn get<T: Setting>(&self) -> &T {
        self.registered
            .get(&TypeId::of::<T>())
            .and_then(|view| view.value.downcast_ref::<T>())
            .unwrap_or_else(|| {
                panic!(
                    "settings type {} is not registered; register it before reading",
                    type_name::<T>()
                )
            })
    }
}

/// A typed view of the settings, built from the merged [`Settings`] in the
/// [`SettingsStore`].
///
/// A feature crate implements [`Setting::from_settings`] for the slice of settings it
/// cares about, calls [`Setting::register`] once, and reads its view back with
/// [`Setting::get_global`]. Only types that register are read from the store.
pub trait Setting: Sized + 'static {
    /// Build this view from the resolved settings.
    fn from_settings(settings: &Settings) -> Self;

    /// Register this type with the store so [`Setting::get_global`] can read it: this is
    /// `register::<T>(cx)` for the type `T` being registered.
    fn register(cx: &mut App) {
        cx.default_global::<SettingsStore>().register::<Self>();
    }

    /// This type's current view of the settings.
    #[track_caller]
    fn get_global(cx: &App) -> &Self {
        cx.global::<SettingsStore>().get::<Self>()
    }
}

impl Setting for Settings {
    /// The resolved settings are their own view.
    fn from_settings(settings: &Settings) -> Self {
        settings.clone()
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "settings_watch_tests.rs"]
mod watch_tests;
