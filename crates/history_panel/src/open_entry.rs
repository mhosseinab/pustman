//! Opening one history entry (PRD REQ-HIST-02): the stored, pre-resolution request —
//! `{{var}}` templates intact — comes back as a request tab, and the stored response
//! summary opens beside it as a read-only tab (status, total time and body size, with
//! no editing controls).
//!
//! `Decision:` the open path is a plain pub function, not a `pustman_actions` action
//! (`pustman_actions::history` holds only `Focus` and this step may not edit that crate)
//! and not a `HistoryList` method (its rows carry the stored entries already): the step
//! that adds row activation wires the rows to [`open_entry`], the shape
//! [`crate::delete_entries`] deferred its buttons under.
//!
//! `Decision:` the summary is its own read-only tab beside the request tab, not a
//! region inside it: this step may not edit `request_editor` (whose tab owns the
//! request draft), so the goal's two things are two pane items. The request tab is
//! added last — `add_tab_item` activates what it adds — so opening an entry lands on
//! the request.

use api_model::HttpRequest;
use gpui::{App, Context, Entity, IntoElement, SharedString, Window};
use request_editor::RequestEditor;
use serde::Deserialize;
use storage::HistoryEntry;
use ui::prelude::*;
use workspace::{Item, Workspace};

/// The one message a failed restore shows. The stored JSON's error text can quote the
/// request it failed on (request content, PRD §7), so it is dropped unread and the user
/// gets this fixed message instead (AGENTS.md — errors reach the user, never only a
/// log).
const OPEN_FAILURE_MESSAGE: &str = "The history entry could not be opened";

/// The one message shown when only the stored response summary is unreadable: the
/// request tab still opens — the summary is secondary — but the failure is never silent.
const SUMMARY_FAILURE_MESSAGE: &str = "The response summary could not be shown";

/// The summary tab's title: it holds the recorded response's facts and nothing else.
const SUMMARY_TAB_TITLE: &str = "Response summary";

/// The summary's stable element id and test hook (the `Welcome` item's pattern).
const SUMMARY_SELECTOR: &str = "history-response-summary";

/// What an unmeasured fact shows: an em dash, never a guess (ADR-0003, the shape
/// `request_editor::send` spells its status facts under).
const UNMEASURED: &str = "—";

/// Open one recorded send in the center pane of `workspace`: a [`RequestEditor`] tab
/// holding the entry's stored request exactly as it was recorded, plus — when the entry
/// has one — the read-only [`ResponseSummary`] tab beside it.
///
/// Everything is synchronous: the caller holds the stored [`HistoryEntry`] (the list
/// already read it), so there is no database work to run off the foreground and no task
/// whose cancellation would have to be reasoned about.
pub fn open_entry(
    entry: &HistoryEntry,
    workspace: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    // `request_json` is the PRE-resolution request (`http_engine` records it that way),
    // so restoring it is a plain deserialize — the `{{var}}` templates travel through
    // untouched and resolve only when the user sends the tab.
    let request = match serde_json::from_value::<HttpRequest>(entry.request_json.clone()) {
        Ok(request) => request,
        Err(_error) => {
            toast(workspace, OPEN_FAILURE_MESSAGE, cx);
            return;
        }
    };
    let summary =
        entry.response_summary_json.as_ref().and_then(
            |document| match ResponseSummary::from_stored(document) {
                Ok(summary) => Some(summary),
                Err(_error) => {
                    toast(workspace, SUMMARY_FAILURE_MESSAGE, cx);
                    None
                }
            },
        );

    let summary_tab = summary.map(|summary| cx.new(|_cx| summary));
    let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
    workspace.update(cx, |workspace, cx| {
        workspace.center_pane().update(cx, |pane, cx| {
            // Summary first: `add_tab_item` activates what it adds, and the request —
            // the thing the user opened the entry for — must be the active tab.
            if let Some(summary_tab) = summary_tab {
                pane.add_tab_item(summary_tab, cx);
            }
            pane.add_tab_item(editor, cx);
        });
    });
}

/// One fixed message for a failure the user must know about (AGENTS.md — errors reach
/// the user, never only a log).
fn toast(workspace: &Entity<Workspace>, message: &'static str, cx: &mut App) {
    workspace.update(cx, |workspace, cx| {
        workspace.show_toast(message, Severity::Error, None, cx);
    });
}

/// The stored response summary as `http_engine` writes it: the status, the timings
/// under `"timings"`, and the body's byte count. Every field is optional — a send that
/// failed or was cancelled stores a summary with none of them — and unknown fields
/// (the stored headers) are ignored here.
#[derive(Debug, Deserialize)]
struct StoredSummary {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    timings: Option<StoredTimings>,
    #[serde(default)]
    body_size: Option<u64>,
}

/// The timing slice of the stored summary; the summary shows the whole exchange's
/// total, the one timing the panel's rows already quote.
#[derive(Debug, Deserialize)]
struct StoredTimings {
    #[serde(default)]
    total_ms: Option<i64>,
}

/// The read-only response summary tab (PRD REQ-HIST-02): the status, total time and
/// body size one recorded response left behind. Read-only by construction — it holds
/// the facts as display text and offers no control that could edit or re-send anything.
pub struct ResponseSummary {
    /// The recorded status code as display text — [`None`] for a send that failed or
    /// was cancelled.
    status_text: Option<SharedString>,
    /// The recorded total time in whole milliseconds (`http_engine` stores whole ms).
    time_text: SharedString,
    /// The recorded body size in bytes (the `console` export spells stored sizes the
    /// same raw way — this is not a byte formatter).
    size_text: SharedString,
}

impl ResponseSummary {
    /// The summary of the stored `response_summary_json` document. A document that does
    /// not match the stored shape is an error for the caller to report — the facts are
    /// never guessed at.
    fn from_stored(document: &serde_json::Value) -> serde_json::Result<Self> {
        let stored: StoredSummary = serde_json::from_value(document.clone())?;
        Ok(Self {
            status_text: stored.status.map(|status| status.to_string().into()),
            time_text: fact_text(stored.timings.and_then(|timings| timings.total_ms), "ms"),
            size_text: fact_text(stored.body_size, "B"),
        })
    }
}

impl Item for ResponseSummary {
    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::new_static(SUMMARY_TAB_TITLE)
    }
}

impl Render for ResponseSummary {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let time_text = self.time_text.clone();
        let size_text = self.size_text.clone();
        h_flex()
            .id(SUMMARY_SELECTOR)
            .debug_selector(|| SUMMARY_SELECTOR.to_owned())
            .w_full()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base03.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            // The status is a `Badge` — the component's own "a status" use case — and
            // its color never carries the meaning alone: the code itself is the text.
            .child(match &self.status_text {
                Some(status_text) => Badge::new(status_text.clone()).into_any_element(),
                None => Label::new(UNMEASURED)
                    .color(Color::Muted)
                    .into_any_element(),
            })
            .child(
                Label::new(time_text)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                Label::new(size_text)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
    }
}

/// One recorded fact as the summary shows it: the value with its unit, or the em dash
/// when the send left it unmeasured (ADR-0003).
fn fact_text(value: Option<impl std::fmt::Display>, unit: &'static str) -> SharedString {
    match value {
        Some(value) => format!("{value} {unit}").into(),
        None => SharedString::new_static(UNMEASURED),
    }
}

#[cfg(test)]
#[path = "open_entry_tests.rs"]
mod tests;
