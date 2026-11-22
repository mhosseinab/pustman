//! Find in the response body (REQ-RESP-03): the find bar [`FindBar`] and the matcher
//! behind it.
//!
//! `Ctrl-F` dispatches `pustman_actions::response::Find` to [`ResponseView::find`],
//! which opens the bar over the body ([`crate::BodyView::open_find`]). The query field
//! is a [`text_input::TextInput`]; two toggles turn on regex matching and case
//! sensitivity, the step buttons (and Enter / Shift-Enter) walk the matches wrapping
//! around, and the count reads "3 of 12" or "No matches". A query that does not compile
//! shows its error alone, inline under the field — the count stays silent beside it,
//! and the error is never only in a log.
//!
//! Matching is the `regex` crate and nothing else: a regex query is compiled as written,
//! a plain-text query goes through [`regex::escape`]. The match list ([`FindMatch`]:
//! shown line + byte range) is computed in [`FindBar::recompute`] whenever the query,
//! the options or the searched lines change — never in `render` — and [`FindEvent`]
//! tells the body to re-highlight the visible rows, scroll the current match into view,
//! or close the bar.
//!
//! `Decision:` Escape and Shift-Enter are key *listeners* on the bar (the
//! `workspace::modal_layer` pattern), not keymap bindings: a find bar's own keys are
//! widget semantics (like `text_input`'s editing keys), and this step defines no new
//! actions or bindings — `response::Find` and its `ctrl-f`/`cmd-f` binding already
//! exist. Enter arrives through the query field's `Submitted` event instead. When the
//! viewer is nested under the `RequestEditor` key context (a later integration step),
//! Escape's precedence there must be revisited so the bar's listener keeps winning.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Subscription, Window,
};
use pustman_actions::response::Find;
use regex::{Regex, RegexBuilder};
use text_input::{TextInput, TextInputEvent};
use ui::prelude::*;

use crate::{BODY_TAB, ResponseView, child_id};

/// One match: the shown line it is on and the byte range it covers there. The list is
/// ordered by line, then range — so a row finds its own matches by binary search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindMatch {
    /// The matched line's index in the searched lines.
    pub line: usize,
    /// The bytes the match covers within that line.
    pub range: Range<usize>,
}

/// What the bar tells the body ([`crate::BodyView`]) to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindEvent {
    /// The match list or the current match changed: re-highlight and scroll.
    Changed,
    /// Escape: the bar goes away and the body takes focus back.
    Dismissed,
}

/// The find bar (REQ-RESP-03): query field, regex and case options, previous/next and
/// the match count. It owns the whole search state — the lines to search
/// ([`Self::set_lines`]), the options, the matches and the current one — so the body
/// pane only draws what it says.
pub struct FindBar {
    /// The query field. Its [`TextInputEvent`]s re-search on every edit and take Enter
    /// to the next match.
    query: Entity<TextInput>,
    /// The query is a regular expression (off: literal text, escaped).
    regex: bool,
    /// Letter case counts (off: case-insensitive).
    case_sensitive: bool,
    /// The shown body lines to search — the same lines the pane draws.
    lines: Arc<Vec<SharedString>>,
    /// Every match, ordered by line then range.
    matches: Arc<Vec<FindMatch>>,
    /// Where "next" and "previous" move from: an index into `matches`.
    current: Option<usize>,
    /// The query did not compile; shown inline under the field.
    error: Option<SharedString>,
    /// Kept so the field's edits keep reaching us; dropping it would silently stop
    /// searching (AGENTS.md › GPUI).
    _query_subscription: Subscription,
}

impl FindBar {
    /// An empty bar over no body: give it lines with [`Self::set_lines`] and put the
    /// cursor in the query field.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(SharedString::new_static("Find in body"))
        });
        // `text_input`'s handle defaults to not being a tab stop; the query is the bar's
        // first keyboard stop (`request_editor::key_value_table` does the same).
        query.read(cx).focus_handle(cx).tab_stop(true);
        let _query_subscription = cx.subscribe_in(&query, window, Self::on_query_event);
        Self {
            query,
            regex: false,
            case_sensitive: false,
            lines: Arc::new(Vec::new()),
            matches: Arc::new(Vec::new()),
            current: None,
            error: None,
            _query_subscription,
        }
    }

    /// Search these lines — call whenever the pane's shown lines change. The match list
    /// recomputes now and [`FindEvent::Changed`] follows.
    pub fn set_lines(&mut self, lines: Arc<Vec<SharedString>>, cx: &mut Context<Self>) {
        self.lines = lines;
        self.recompute(cx);
    }

    /// Treat the query as a regular expression; `false` searches literal text.
    pub fn set_regex(&mut self, regex: bool, cx: &mut Context<Self>) {
        if self.regex == regex {
            return;
        }
        self.regex = regex;
        self.recompute(cx);
    }

    /// Match letter case exactly; `false` searches case-insensitively.
    pub fn set_case_sensitive(&mut self, case_sensitive: bool, cx: &mut Context<Self>) {
        if self.case_sensitive == case_sensitive {
            return;
        }
        self.case_sensitive = case_sensitive;
        self.recompute(cx);
    }

    /// How many matches the query in effect has.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Every match, ordered by line then range — what the body highlights.
    pub fn matches(&self) -> Arc<Vec<FindMatch>> {
        Arc::clone(&self.matches)
    }

    /// The line the current match is on, for the body to scroll into view.
    pub fn current_line(&self) -> Option<usize> {
        self.matches.get(self.current?).map(|found| found.line)
    }

    /// What the count beside the controls says: "3 of 12", "No matches", or nothing
    /// before the first query and while the query does not compile — there the inline
    /// error speaks alone (#161).
    pub fn count_label(&self, cx: &App) -> Option<SharedString> {
        if self.query.read(cx).text().is_empty() || self.error.is_some() {
            return None;
        }
        let Some(current) = self.current else {
            return Some(SharedString::new_static("No matches"));
        };
        let position = current + 1;
        let total = self.match_count();
        Some(format!("{position} of {total}").into())
    }

    /// Recompute the match list from the query in effect and the searched lines. This is
    /// the only place matches are born: a query edit, an option toggle and new lines all
    /// land here, and `render` never computes anything. A new query starts at the first
    /// match (or none, when it does not compile).
    fn recompute(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).text();
        self.matches = Arc::new(Vec::new());
        self.current = None;
        self.error = None;
        if !query.is_empty() {
            match compile_query(&query, self.regex, self.case_sensitive) {
                Ok(pattern) => {
                    let matches = find_matches(&self.lines, &pattern);
                    self.current = (!matches.is_empty()).then_some(0);
                    self.matches = Arc::new(matches);
                }
                Err(error) => {
                    self.error = Some(format!("Not a valid pattern: {error}").into());
                }
            }
        }
        cx.emit(FindEvent::Changed);
    }

    /// Move to the next or previous match, wrapping around both ends (REQ-RESP-03).
    fn move_current(&mut self, forward: bool, cx: &mut Context<Self>) {
        let total = self.matches.len();
        if total == 0 {
            return;
        }
        let current = self.current.unwrap_or(0);
        self.current = Some(if forward {
            (current + 1) % total
        } else {
            (current + total - 1) % total
        });
        cx.emit(FindEvent::Changed);
    }

    /// The query field: every edit re-searches, and Enter goes to the next match.
    fn on_query_event(
        &mut self,
        _query: &Entity<TextInput>,
        event: &TextInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextInputEvent::Changed => self.recompute(cx),
            TextInputEvent::Submitted => self.move_current(true, cx),
        }
    }

    /// The bar's own keys: Escape closes it, Shift-Enter goes to the previous match.
    /// Both stop propagating so an ancestor does not answer as well — Escape elsewhere
    /// in the app cancels a send (`assets/keymaps/`).
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match (
            event.keystroke.key.as_str(),
            event.keystroke.modifiers.shift,
        ) {
            ("escape", _) => {
                cx.stop_propagation();
                cx.emit(FindEvent::Dismissed);
            }
            ("enter", true) => {
                cx.stop_propagation();
                self.move_current(false, cx);
            }
            _ => {}
        }
    }

    /// The one row of controls: query field, options, previous/next, count.
    fn render_controls(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .items_center()
            .gap(DynamicSpacing::Base03.rems(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(DynamicSpacing::Base03.rems(cx))
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_sm()
                    .child(self.query.clone()),
            )
            .child(self.render_regex_toggle(base, cx))
            .child(self.render_case_toggle(base, cx))
            .child(self.render_step_button(
                base,
                "previous-match",
                IconName::ChevronUp,
                "Previous match (Shift+Enter)",
                Self::previous_match,
                cx,
            ))
            .child(self.render_step_button(
                base,
                "next-match",
                IconName::ChevronDown,
                "Next match (Enter)",
                Self::next_match,
                cx,
            ))
            .children(
                self.count_label(cx)
                    .map(|label| Label::new(label).size(LabelSize::Small).color(Color::Muted)),
            )
            .into_any_element()
    }

    /// The regex option. The label is the control's accessible name (`Toggle`).
    fn render_regex_toggle(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        Toggle::new(
            child_id(base, "toggle-regex"),
            SharedString::new_static("Regex"),
            ToggleState::from(self.regex),
        )
        .on_change(move |state, _window, cx| {
            if weak
                .update(cx, |bar, cx| bar.set_regex(state.is_selected(), cx))
                .is_err()
            {
                // Handled: the bar went away with its window while the toggle was up;
                // there is nothing left to search.
            }
        })
        .into_any_element()
    }

    /// The case-sensitivity option.
    fn render_case_toggle(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        Toggle::new(
            child_id(base, "toggle-case"),
            SharedString::new_static("Case sensitive"),
            ToggleState::from(self.case_sensitive),
        )
        .on_change(move |state, _window, cx| {
            if weak
                .update(cx, |bar, cx| {
                    bar.set_case_sensitive(state.is_selected(), cx)
                })
                .is_err()
            {
                // Handled: the bar went away with its window while the toggle was up;
                // there is nothing left to search.
            }
        })
        .into_any_element()
    }

    /// One step button. `step` is which way the wrap goes ([`Self::previous_match`] or
    /// [`Self::next_match`]); the tooltip names the key that does the same.
    fn render_step_button(
        &self,
        base: &ElementId,
        name: &str,
        icon: IconName,
        tooltip: &str,
        step: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        IconButton::new(child_id(base, name), icon, Tooltip::text(tooltip))
            .size(ButtonSize::Medium)
            .on_click(move |_event, _window, cx| {
                if weak.update(cx, step).is_err() {
                    // Handled: the bar went away with its window while the button was
                    // up; there is nothing left to step through.
                }
            })
            .into_any_element()
    }

    fn next_match(&mut self, cx: &mut Context<Self>) {
        self.move_current(true, cx);
    }

    fn previous_match(&mut self, cx: &mut Context<Self>) {
        self.move_current(false, cx);
    }
}

impl Render for FindBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One base id per bar, from the query's stable focus handle: two viewers in one
        // frame never share element ids.
        let base = ElementId::from(&self.focus_handle(cx));
        v_flex()
            .id(child_id(&base, "find-bar"))
            .w_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base02.rems(cx))
            .gap(DynamicSpacing::Base02.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(self.render_controls(&base, cx))
            .map(|this| match &self.error {
                Some(message) => this.child(
                    Label::new(message.clone())
                        .size(LabelSize::Small)
                        .color(Color::Error),
                ),
                None => this,
            })
    }
}

impl Focusable for FindBar {
    /// The bar is one tab stop: it delegates focus to the query field, so typing works
    /// the moment the bar opens (the `Picker` and `HistoryList` contract).
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.read(cx).focus_handle(cx)
    }
}

impl EventEmitter<FindEvent> for FindBar {}

/// Compile `text` as the query in effect: a regex as written, or literal text through
/// [`regex::escape`] — the one matcher for both modes.
fn compile_query(text: &str, regex: bool, case_sensitive: bool) -> Result<Regex, regex::Error> {
    let pattern = if regex {
        text.to_owned()
    } else {
        regex::escape(text)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
}

/// Every match of `pattern` in `lines`, ordered by line then range. Zero-width matches
/// are dropped: they highlight nothing and would pad the count.
fn find_matches(lines: &[SharedString], pattern: &Regex) -> Vec<FindMatch> {
    let mut matches = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        for found in pattern.find_iter(text) {
            if found.is_empty() {
                continue;
            }
            matches.push(FindMatch {
                line,
                range: found.start()..found.end(),
            });
        }
    }
    matches
}

impl ResponseView {
    /// `response::Find` (REQ-RESP-03): open the find bar over the body. Find is a body
    /// feature, so the Body tab comes up first — the bar is invisible anywhere else.
    pub(crate) fn find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_tab = BODY_TAB;
        self.tab_bar.update(cx, |bar, cx| {
            if bar.selected() != BODY_TAB {
                bar.set_selected(BODY_TAB, cx);
            }
        });
        self.body.update(cx, |body, cx| body.open_find(window, cx));
        cx.notify();
    }
}

#[cfg(test)]
#[path = "find_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "find_error_tests.rs"]
mod error_tests;

#[cfg(test)]
#[path = "find_keyboard_tests.rs"]
mod keyboard_tests;
