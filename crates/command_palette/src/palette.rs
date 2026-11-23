//! The command palette (PRD REQ-UI-05): every action the user can dispatch where they
//! were, with the keys the active keymap gives it, in one fuzzy-searchable modal.
//!
//! `Decision:` the palette is a [`Picker`] with a [`PaletteDelegate`], hosted by the
//! workspace's modal layer (pack screen S15 "Search and command overlay", requirement
//! R15): each row is the action's humanized name and its live [`KeyBinding`] hint, and
//! confirming a row dispatches that action after the modal closes so it travels from the
//! focus the modal restored. The picker draws the surface; this module is only the list.
//!
//! `Decision:` `palette::Toggle` is answered by an app-level handler ([`init`]) and not
//! by an element listener: `Workspace` registers actions in its own `render` and this
//! crate may not edit `crates/workspace` or `crates/pustman`, so there is no element of
//! ours to hang a listener on. The handler collects [`Window::available_actions`] before
//! the palette takes focus, so the list is what the user could dispatch where they were
//! and not what the query field can.

use gpui::{Action, App, Context, DismissEvent, ElementId, Global, SharedString, Window};
use pustman_actions::palette;
use ui::prelude::*;
use workspace::{ModalView, Workspace};

use crate::picker::{self, Picker, PickerDelegate};

/// One palette row: the action to dispatch and the name it is listed under. The
/// keybinding hint is read from the keymap at render time ([`Self::binding_text`]), so it
/// cannot go stale when the user rebinds a key.
pub struct PaletteEntry {
    action: Box<dyn Action>,
    name: String,
}

impl PaletteEntry {
    /// The name the row shows: `request::Send` → "Request: Send".
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The action confirming this row dispatches.
    pub fn action(&self) -> &dyn Action {
        self.action.as_ref()
    }

    /// The keys the active keymap binds to this action on this platform, spelled the way
    /// [`KeyBinding`] renders them ([`None`] when nothing is bound).
    pub fn binding_text(&self, window: &Window, cx: &App) -> Option<String> {
        KeyBinding::for_action(self.action.as_ref()).keystroke_text(window, cx)
    }
}

/// How recently each action was confirmed from the palette: the rows the palette opens
/// with list the most recently used actions first (pack S15/R15; `postman-screens.md`:
/// "Recently used actions first"), then the rest alphabetically.
///
/// `Decision:` recency is a per-session app [`Global`], not persisted. It only reorders
/// rows the query and keymap already order otherwise, so it is a convenience rather
/// than user data; persisting it is a `storage` decision for a later step.
#[derive(Default)]
struct PaletteUsage {
    recent: Vec<String>,
}

impl Global for PaletteUsage {}

impl PaletteUsage {
    /// Note that `action_name` was just used: it moves to the front of the recency list.
    fn record(&mut self, action_name: &str) {
        self.recent.retain(|used| used != action_name);
        self.recent.insert(0, action_name.to_owned());
    }

    /// How recently `action_name` was used: 0 is the most recent, [`NEVER_USED`] when it
    /// was never confirmed.
    fn recency(&self, action_name: &str) -> usize {
        self.recent
            .iter()
            .position(|used| used == action_name)
            .unwrap_or(NEVER_USED)
    }
}

/// The recency of an action the palette has recorded nothing about: it sorts behind
/// every used action.
const NEVER_USED: usize = usize::MAX;

/// The recency of `action_name` under `usage` — [`NEVER_USED`] before the palette has
/// recorded anything.
fn recency_of(usage: Option<&PaletteUsage>, action_name: &str) -> usize {
    usage.map_or(NEVER_USED, |usage| usage.recency(action_name))
}

/// The delegate behind the command palette: every dispatchable action, most recently
/// used first and then sorted by name, fuzzy-filtered by [`crate::fuzzy`] against the
/// query.
pub struct PaletteDelegate {
    entries: Vec<PaletteEntry>,
    matches: Vec<usize>,
    selected: usize,
}

impl PaletteDelegate {
    /// The palette over `actions` — everything the user could dispatch where they were,
    /// minus the palette's own mechanics, in row order: the most recently used actions
    /// first (pack S15/R15), the rest alphabetically so the list is deterministic.
    pub fn new(actions: impl IntoIterator<Item = Box<dyn Action>>, cx: &App) -> Self {
        let hidden = hidden_action_names();
        let usage = cx.try_global::<PaletteUsage>();
        let mut entries: Vec<PaletteEntry> = actions
            .into_iter()
            .filter(|action| !hidden.contains(&action.name()))
            .map(|action| PaletteEntry {
                name: humanize_name(action.name()),
                action,
            })
            .collect();
        entries.sort_by(|left, right| {
            // A never-used action's [`NEVER_USED`] recency sorts it behind every use,
            // so the used actions lead, most recent first.
            recency_of(usage, left.action.name())
                .cmp(&recency_of(usage, right.action.name()))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.action.name().cmp(right.action.name()))
        });
        let matches = (0..entries.len()).collect();
        Self {
            entries,
            matches,
            selected: 0,
        }
    }

    /// Every entry, in the order the palette lists them.
    pub fn entries(&self) -> &[PaletteEntry] {
        &self.entries
    }

    /// The entry behind match `index` — [`None`] when the row is out of range.
    fn entry_at(&self, index: usize) -> Option<&PaletteEntry> {
        self.matches
            .get(index)
            .and_then(|entry_index| self.entries.get(*entry_index))
    }
}

impl PickerDelegate for PaletteDelegate {
    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected
    }

    fn set_selected_index(&mut self, index: usize, cx: &mut Context<Picker<Self>>) {
        self.selected = index;
        cx.notify();
    }

    fn update_matches(&mut self, query: &str, cx: &mut Context<Picker<Self>>) {
        self.matches =
            crate::fuzzy::rank(query, self.entries.iter().map(|entry| entry.name.as_str()))
                .into_iter()
                .map(|string_match| string_match.candidate_index)
                .collect();
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(entry) = self.entry_at(self.selected) else {
            return;
        };
        // Confirming is where the palette learns what the user actually picks: the
        // action leads the next palette's rows (pack S15/R15).
        cx.default_global::<PaletteUsage>()
            .record(entry.action.name());
        let action = entry.action.boxed_clone();
        // Dismiss first: the modal layer restores focus as it goes. The dispatch is one
        // effect later (`window.defer`), so it runs against the *restored* focus and the
        // action reaches the handler it would reach without the palette.
        cx.emit(DismissEvent);
        window.defer(cx, move |window, cx| {
            window.dispatch_action(action, cx);
        });
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        index: usize,
        _selected: bool,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let Some(entry) = self.entry_at(index) else {
            return div().into_any_element();
        };
        h_flex()
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                div()
                    .id(format!("palette-name-{}", entry.action.name()))
                    .min_w_0()
                    .flex_1()
                    .tooltip(Tooltip::text(entry.name.clone()).builder())
                    .child(Label::new(entry.name.clone()).truncate()),
            )
            .child(KeyBinding::for_action(entry.action.as_ref()))
            .into_any_element()
    }

    fn match_key(&self, index: usize) -> ElementId {
        // The action name is the row's identity — never its rank, which the query changes.
        self.entry_at(index)
            .map(|entry| ElementId::from(format!("palette-action-{}", entry.action.name())))
            .unwrap_or_else(|| ElementId::from("palette-row"))
    }

    fn placeholder_text(&self) -> SharedString {
        SharedString::new_static("Type a command")
    }
}

/// The name a row shows for an action: the namespace and the struct name, each split
/// into words and capitalized — `request::SendRequest` → "Request: Send Request".
fn humanize_name(action_name: &str) -> String {
    action_name
        .split("::")
        .map(humanize_segment)
        .collect::<Vec<_>>()
        .join(": ")
}

/// `SendRequest`, `send_request` or `URLBar` → "Send Request", "Send Request", "URL Bar".
fn humanize_segment(segment: &str) -> String {
    segment
        .split(['_', ' '])
        .filter(|word| !word.is_empty())
        .flat_map(split_camel_case)
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The words a camelCase run is made of: `SendURLBar` → ["Send", "URL", "Bar"]. A word
/// starts at a capital that follows a lowercase or numeric character, or at the capital
/// that opens a lowercase run (`SendURL|Bar`).
fn split_camel_case(segment: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let mut characters = segment.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if index > start {
            let next = characters.peek().map(|(_, character)| *character);
            let opens_word =
                previous.is_some_and(|previous| previous.is_lowercase() || previous.is_numeric());
            let ends_acronym =
                previous.is_some_and(char::is_uppercase) && next.is_some_and(char::is_lowercase);
            if character.is_uppercase() && (opens_word || ends_acronym) {
                words.push(&segment[start..index]);
                start = index;
            }
        }
        previous = Some(character);
    }
    if start < segment.len() {
        words.push(&segment[start..]);
    }
    words
}

/// The word with its first character uppercased: `request` → `Request`.
fn capitalize_word(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

/// The actions the palette hides: the picker's own navigation actions and the toggle
/// that opened it. They are dispatchable from where the user is, but listing them would
/// offer the palette to itself.
fn hidden_action_names() -> [&'static str; 5] {
    [
        picker::SelectNext::name_for_type(),
        picker::SelectPrevious::name_for_type(),
        picker::Confirm::name_for_type(),
        picker::Cancel::name_for_type(),
        palette::Toggle::name_for_type(),
    ]
}

/// Wire the palette to `palette::Toggle` (PRD REQ-UI-05): open it over the active
/// workspace — or close it, [`Workspace::toggle_modal`] toggles — listing what the user
/// can dispatch where they are.
pub fn init(cx: &mut App) {
    cx.on_action(toggle_palette);
}

/// Open the palette in the active window. `palette::Toggle` reaches this through GPUI's
/// app-level action listeners: the dispatch happens inside the window's own update and a
/// window cannot be updated while it is on the update stack, so the toggle is deferred —
/// the shape Zed's `with_active_or_new_workspace` uses for the same situation.
fn toggle_palette(_: &palette::Toggle, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        match window_handle.update(cx, |root, window, cx| {
            let Ok(workspace) = root.downcast::<Workspace>() else {
                return;
            };
            // Collected before the palette takes focus: the list is what the user
            // could dispatch where they were, not what the query field can.
            let actions = window.available_actions(cx);
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, |window, cx| {
                    Picker::new(PaletteDelegate::new(actions, cx), window, cx)
                });
            });
        }) {
            Ok(()) => {}
            // The window closed between the keystroke and this callback: there is no
            // workspace left to open the palette over and nobody left to tell, so this
            // toggle is dropped here on purpose — the discard is spelled out, not a
            // result silently dropped on the floor.
            Err(_window_gone) => {}
        }
    });
}

/// A picker is a modal: it renders, takes focus and says when it is done by emitting
/// [`DismissEvent`] — the [`ModalView`] contract of `crates/workspace/src/modal_layer.rs`.
impl<D: PickerDelegate> ModalView for Picker<D> {}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "palette_recent_tests.rs"]
mod recent_tests;
