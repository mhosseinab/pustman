//! Opening one history entry (PRD REQ-HIST-02): the stored, pre-resolution request —
//! `{{var}}` templates intact — comes back as a request tab, and the recorded response
//! shows in that tab's own response region, the way a fresh send would show it.
//!
//! `Decision:` the open path is a plain pub function, not a `pustman_actions` action
//! (`pustman_actions::history` holds only `Focus`) and not a `HistoryList` method (its
//! rows carry the stored entries already).
//!
//! `Decision:` the response goes into the request tab's embedded response view rather
//! than a second, read-only tab: the separate tab repeated what that region shows and
//! doubled the tabs each open added. History stores a summary: the body is the stored
//! prefix (at most `http_engine::HISTORY_BODY_LIMIT`), and no reason phrase is kept, so
//! the status's standard phrase stands in. A send that failed or was cancelled stored no
//! response, and none is shown.

use std::time::Duration;

use api_model::{HttpRequest, ItemId};
use gpui::{App, Entity, Window};
use http_engine::{Header, Response, ResponseBody, Timings};
use request_editor::{HistoryOrigin, RequestEditor};
use serde::Deserialize;
use storage::HistoryEntry;
use ui::prelude::*;
use workspace::Workspace;

/// The one message a failed restore shows. The stored JSON's error text can quote the
/// request it failed on (request content, PRD §7), so it is dropped unread and the user
/// gets this fixed message instead (AGENTS.md — errors reach the user, never only a
/// log).
const OPEN_FAILURE_MESSAGE: &str = "The history entry could not be opened";

/// The one message shown when only the stored response is unreadable: the request tab
/// still opens — the response is secondary — but the failure is never silent.
const SUMMARY_FAILURE_MESSAGE: &str = "The recorded response could not be shown";

/// Open one recorded send in the center pane of `workspace`: one [`RequestEditor`] tab
/// holding the entry's stored request exactly as it was recorded, with the recorded
/// response — when the entry has one — in the tab's response region.
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
    open_entry_recorded_in(entry, None, workspace, window, cx);
}

/// [`open_entry`] for an entry recorded in the folder whose installation key is
/// `installation_key` (`None`: none known, as for every row recorded before #334). With a
/// key the tab is built by [`RequestEditor::from_history`], carrying that key and the
/// request id it was recorded under, so the scope wire can tell whether the folder open
/// now is the one it was sent from; without one it is a plain tab, as before.
pub fn open_entry_recorded_in(
    entry: &HistoryEntry,
    installation_key: Option<&str>,
    workspace: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    // `request_json` is the PRE-resolution request (`http_engine` records it that way),
    // so restoring it is a plain deserialize — the `{{var}}` templates travel through
    // untouched and resolve only when the user sends the tab.
    let (request, recorded_id) =
        match serde_json::from_value::<HttpRequest>(entry.request_json.clone()) {
            // A fresh identity: the recorded one may already name an open tab (the
            // request itself, or this entry opened before), and two tabs with one
            // identity make the workspace's tab save fail. The recorded one goes along
            // as the tab's origin.
            Ok(request) => {
                let recorded_id = request.id;
                (
                    HttpRequest {
                        id: ItemId::new(),
                        ..request
                    },
                    recorded_id,
                )
            }
            Err(_error) => {
                toast(workspace, OPEN_FAILURE_MESSAGE, cx);
                return;
            }
        };
    let response = entry
        .response_summary_json
        .as_ref()
        .and_then(|document| match stored_response(document, entry) {
            Ok(response) => response,
            Err(_error) => {
                toast(workspace, SUMMARY_FAILURE_MESSAGE, cx);
                None
            }
        });

    let origin = installation_key.map(|installation_key| HistoryOrigin {
        installation_key: installation_key.to_owned(),
        recorded_id,
    });
    let editor = cx.new(|cx| match origin {
        Some(origin) => RequestEditor::from_history(request, origin, window, cx),
        None => RequestEditor::new(request, window, cx),
    });
    if let Some(response) = response {
        editor.update(cx, |editor, cx| {
            editor
                .response_view()
                .update(cx, |view, cx| view.set_response(response, cx));
        });
    }
    workspace.update(cx, |workspace, cx| {
        workspace
            .center_pane()
            .update(cx, |pane, cx| pane.add_tab_item(editor, cx));
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
/// under `"timings"` and the (already redacted) headers. A send that failed or was
/// cancelled stores a summary with no status. Unknown fields are ignored.
#[derive(Debug, Deserialize)]
struct StoredSummary {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    timings: Option<StoredTimings>,
    #[serde(default)]
    headers: Vec<StoredHeader>,
}

/// The stored timings, whole milliseconds each; `null` for a phase never measured.
#[derive(Debug, Default, Deserialize)]
struct StoredTimings {
    #[serde(default)]
    prepare_ms: Option<u64>,
    #[serde(default)]
    dns_ms: Option<u64>,
    #[serde(default)]
    connect_ms: Option<u64>,
    #[serde(default)]
    tls_ms: Option<u64>,
    #[serde(default)]
    first_byte_ms: Option<u64>,
    #[serde(default)]
    download_ms: Option<u64>,
    #[serde(default)]
    total_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct StoredHeader {
    name: String,
    value: String,
}

/// The response the entry recorded, rebuilt from its summary and stored body prefix;
/// [`None`] when the send never got one. A document that does not match the stored
/// shape is an error for the caller to report — the facts are never guessed at.
fn stored_response(
    document: &serde_json::Value,
    entry: &HistoryEntry,
) -> serde_json::Result<Option<Response>> {
    let stored: StoredSummary = serde_json::from_value(document.clone())?;
    let Some(status) = stored.status else {
        return Ok(None);
    };
    let body = entry.response_body_prefix.clone().unwrap_or_default();
    // History keeps no reason phrase, so the status code's standard one stands in.
    let reason = reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|code| code.canonical_reason())
        .unwrap_or_default();
    let mut response = Response::new(status, reason, ResponseBody::Bytes(body));
    response.headers = stored
        .headers
        .into_iter()
        .map(|header| Header::new(header.name, header.value))
        .collect();
    let timings = stored.timings.unwrap_or_default();
    let millis = |value: Option<u64>| value.map(Duration::from_millis);
    response.timings = Timings {
        prepare: millis(timings.prepare_ms),
        dns: millis(timings.dns_ms),
        connect: millis(timings.connect_ms),
        tls: millis(timings.tls_ms),
        first_byte: millis(timings.first_byte_ms),
        download: millis(timings.download_ms),
        total: millis(timings.total_ms),
    };
    Ok(Some(response))
}

#[cfg(test)]
#[path = "open_entry_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "open_entry_identity_tests.rs"]
mod identity_tests;
