//! A secret variable's value: the masked field the user types it into and the OS keychain
//! that keeps it — never the environment file and never `storage` (PRD REQ-VAR-SHARE-02).
//!
//! The editor's secret rows hold no shared value and no local value; their one value field
//! is [`make_secret_field`]'s masked one, and its text is set and read through
//! [`SecretStore`] only, on background tasks: [`read_secret`] at open and
//! [`SecretChange::apply`] at save (`env_editor` runs both off the foreground). The file
//! records a secret's key, kind and description only, and the local store holds nothing
//! for a secret at all.
//!
//! # Invariants
//!
//! - A secret's value never reaches a workspace file, SQLite, `Debug`/`Display` output, a
//!   log, a toast, an error message or the Console. [`SecretValue`] prints `<redacted>`,
//!   [`SecretChange`] implements no `Debug` or `Display` at all, the reasons
//!   [`SecretState::Unreadable`] carries are [`SecretError`]'s own (keys and platform
//!   diagnostics, never values), and the masked field paints one stand-in per character
//!   (`TextInput::display_text`) with no tooltip of its own text.
//! - A keychain failure is shown to the user — [`SecretState::Unreadable`] at open,
//!   [`SecretChange::apply`]'s error at save — and is never worked around by keeping the
//!   value somewhere else: with no keychain connected ([`install_inputs`] never ran) a
//!   write is refused before anything is saved, never dropped and never redirected into a
//!   fallback store or the file.
//!
//! `Decision:` the keychain is an app-global ([`SecretInputs`]) the app's wire installs
//! with [`install_inputs`] — the same "the top crate owns what this crate cannot know"
//! shape `env_editor::EnvEditorInputs` and `quick_look::QuickLookInputs` use — with **no**
//! default store on purpose. A run that never connected a keychain must fail loudly
//! ("the OS keychain is not connected"), not silently keep secrets in memory where a quit
//! loses them, and not fall back to a file; the wire that installs the editor's inputs
//! installs this too.
//!
//! `Decision:` this module reads and writes the keychain but never decides *when*: the
//! editor's open snapshot runs [`read_secret`] off the foreground and one save's plan
//! carries its [`SecretChange`]s, so the keychain is touched on background tasks only and
//! a save that is refused as stale writes nothing anywhere — keychain included.

use std::fmt;
use std::sync::Arc;

use gpui::{App, Entity, Focusable, Global, Window};
use secrets::{SecretError, SecretKey, SecretStore, SecretValue};
use text_input::TextInput;
use ui::prelude::*;

/// The placeholder of an empty secret field — the same "an example, not a value" shape
/// `env_editor`'s other fields use, and obviously fake (`.agents/rules/tests.md`).
const SECRET_PLACEHOLDER: &str = "e.g. token-REDACTED-example";

/// Why every read and write fails when the app's wire never installed a keychain
/// ([`install_inputs`]): named in the message the user sees, never worked around.
pub const NOT_CONNECTED: &str = "the OS keychain is not connected";

/// The keychain the editor keeps secret values in — what one editor session cannot know
/// on its own. See the module docs for why there is no default store.
#[derive(Clone)]
pub struct SecretInputs {
    /// The keychain the secret values live in.
    pub store: Arc<dyn SecretStore>,
}

impl Global for SecretInputs {}

/// Install the keychain the next editor runs with.
pub fn install_inputs(cx: &mut App, inputs: SecretInputs) {
    cx.set_global(inputs);
}

/// The keychain the editor keeps secret values in, when one is connected (module docs).
pub fn store(cx: &App) -> Option<Arc<dyn SecretStore>> {
    cx.try_global::<SecretInputs>()
        .map(|inputs| Arc::clone(&inputs.store))
}

/// The masked value field of a secret row (module docs): the painted line and the platform
/// read one stand-in per character of the value it holds (`TextInput::with_masked`), never
/// the value itself. `text` is what the keychain read found — empty when there is nothing
/// set — and the field is a tab stop like every other cell's field.
pub fn make_secret_field(text: &str, window: &mut Window, cx: &mut App) -> Entity<TextInput> {
    let field = cx.new(|cx| {
        TextInput::new(window, cx)
            .with_placeholder(SharedString::new_static(SECRET_PLACEHOLDER))
            .with_masked()
    });
    field.read(cx).focus_handle(cx).tab_stop(true);
    if !text.is_empty() {
        field.update(cx, |field, cx| field.set_text(text, cx));
    }
    field
}

/// What one secret variable's keychain entry looked like at the open-time read.
pub struct SecretSnapshot {
    /// The variable key the entry lives under: the row that opened with this key.
    pub key: String,
    /// What the read found.
    pub state: SecretState,
}

/// What a secret's keychain read found (module docs).
pub enum SecretState {
    /// No entry is set: REQ-VAR-SHARE-02's "unresolved secret". The masked field opens
    /// empty, and an empty field with no known entry saves nothing at all.
    Unset,
    /// An entry is set: the masked field opens on its value, masked. The plaintext lives
    /// in the field only — the one place the user could type it themselves.
    Found(SecretValue),
    /// The keychain could not be read. The reason is shown to the user and the field opens
    /// empty — an entry this editor could not read is never guessed at and never deleted
    /// by an empty save.
    Unreadable(String),
}

/// Read one secret variable's keychain entry — a blocking [`SecretStore`] call the caller
/// runs on a background task (module docs). `None` for the store is the not-connected
/// failure of [`NOT_CONNECTED`], folded in here so every read answers the same way.
pub fn read_secret(
    store: Option<&Arc<dyn SecretStore>>,
    workspace_id: &str,
    scope_id: &str,
    variable_key: &str,
) -> SecretSnapshot {
    let state = match store {
        None => SecretState::Unreadable(NOT_CONNECTED.to_string()),
        Some(store) => match SecretKey::new(workspace_id, scope_id, variable_key) {
            Ok(key) => match store.get(&key) {
                Ok(value) => SecretState::Found(value),
                Err(SecretError::NotFound { .. }) => SecretState::Unset,
                Err(error) => SecretState::Unreadable(error.to_string()),
            },
            Err(error) => SecretState::Unreadable(error.to_string()),
        },
    };
    SecretSnapshot {
        key: variable_key.to_string(),
        state,
    }
}

/// One keychain write a save performs: a secret's value set under its key, or an entry
/// removed.
///
/// Carries a [`SecretValue`], so it implements no `Debug` and no `Display` — nothing that
/// may ever be formatted into a message can hold a secret (AGENTS.md › Security).
pub enum SecretChange {
    /// Store `value` under `key`, replacing whatever is there.
    Set { key: SecretKey, value: SecretValue },
    /// Remove the entry under `key`.
    Remove { key: SecretKey },
}

impl SecretChange {
    /// The change that stores `value` — the masked field's text, kept exact and untrimmed
    /// like any password — under `workspace_id`/`scope_id`/`variable_key`.
    ///
    /// # Errors
    ///
    /// [`SecretError::InvalidKey`] when the key is not representable in the OS keychain,
    /// [`SecretError::InvalidValue`] when `value` is empty: an empty value is
    /// indistinguishable from an unset one, so the editor never asks for one.
    pub fn set(
        workspace_id: &str,
        scope_id: &str,
        variable_key: &str,
        value: &str,
    ) -> Result<Self, SecretError> {
        let key = SecretKey::new(workspace_id, scope_id, variable_key)?;
        let value = SecretValue::new(value);
        if value.expose().is_empty() {
            return Err(SecretError::InvalidValue { key });
        }
        Ok(Self::Set { key, value })
    }

    /// The change that removes the entry under `workspace_id`/`scope_id`/`variable_key`.
    ///
    /// # Errors
    ///
    /// [`SecretError::InvalidKey`] when the key is not representable in the OS keychain.
    pub fn remove(
        workspace_id: &str,
        scope_id: &str,
        variable_key: &str,
    ) -> Result<Self, SecretError> {
        Ok(Self::Remove {
            key: SecretKey::new(workspace_id, scope_id, variable_key)?,
        })
    }

    /// Apply this change to the keychain — a blocking [`SecretStore`] call the caller runs
    /// on a background task (module docs). An error names the key and the platform's
    /// reason, never the value.
    pub fn apply(&self, store: &Arc<dyn SecretStore>) -> Result<(), SecretError> {
        match self {
            Self::Set { key, value } => store.set(key, value),
            Self::Remove { key } => store.delete(key),
        }
    }
}

impl fmt::Debug for SecretChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Set { key, .. } => f.debug_tuple("Set").field(key).finish(),
            Self::Remove { key } => f.debug_tuple("Remove").field(key).finish(),
        }
    }
}

#[cfg(test)]
#[path = "secret_values_tests.rs"]
mod tests;
