//! History recording (PRD §8.5, REQ-HIST-01..03): every send — success, error or
//! cancel — leaves one row built from the redacted, pre-resolution request, a redacted
//! response summary and a body prefix capped at one megabyte.
//!
//! The request in the row is the one the user wrote: `{{variables}}` stay as written
//! (REQ-HIST-02), and every text runs through the shared `api_model` redaction before it
//! is stored (REQ-HIST-03, REQ-NFR-04 — one shared `redact`, never a second masker). The
//! send's secret values mask what the response carried; on a cancel they are unknown, so
//! an empty [`SecretValues`] plus the header and auth rules redact the draft — the
//! pre-resolution request holds `{{refs}}`, not resolved values.
//!
//! `storage` is synchronous (ADR-0002): every SQLite call runs inside the background
//! task that records the send, and this module logs nothing with content in it — a
//! failed write is one fixed message whatever failed, because `storage`'s error text can
//! quote the row it failed on and a row is request content (PRD §7).

use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{Auth, HttpRequest, SecretValues, redact_headers, redact_text, redact_url};
use serde_json::{Value, json};
use storage::{HistoryEntry, LocalStore};
use time::OffsetDateTime;
use variables::{ScopeStack, spans};

use crate::error::TransportError;
use crate::pipeline::PipelineError;
use crate::response::{Response, ResponseBody};
use crate::retention::{
    DEFAULT_HISTORY_RETENTION_DAYS, RetentionGate, RetentionWindow, prune_once,
};

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "history_redaction_tests.rs"]
mod redaction_tests;

#[cfg(test)]
#[path = "history_window_tests.rs"]
mod window_tests;

/// How much of a response body one history row keeps (REQ-HIST-02's default). Making it
/// configurable — and `0` disabling storage — is settings work (S16b), not this step.
pub const HISTORY_BODY_LIMIT: usize = 1024 * 1024;

/// The `protocol` column of a send through [`crate::HttpClient::send_request`]: an HTTP
/// exchange (plan D7 gives later protocols their own row shapes).
const PROTOCOL: &str = "http";

/// Records a workspace's request history in `storage`'s `history` table (REQ-HIST-01).
///
/// A client without a recorder records nothing. `Debug` prints no store contents: the
/// store holds every recorded row, and rows are request content.
#[derive(Clone)]
pub struct HistoryRecorder {
    store: Arc<Mutex<LocalStore>>,
    workspace_id: String,
    retention_gate: Arc<RetentionGate>,
    /// The retention window (REQ-HIST-04), shared with every clone of this recorder —
    /// the drafts of sends already in flight hold clones — so
    /// [`HistoryRecorder::set_retention_days`] reaches all of them at once (issue #196).
    retention_days: RetentionWindow,
    /// Where a failed retention prune is reported (AGENTS.md: errors reach the user).
    /// `None` keeps the log-only floor of a caller that installs no report.
    prune_failure_report: Option<Arc<dyn Fn(&'static str) + Send + Sync>>,
}

impl HistoryRecorder {
    /// A recorder writing `workspace_id`'s rows through `store`, pruning history older
    /// than [`DEFAULT_HISTORY_RETENTION_DAYS`] days —
    /// [`HistoryRecorder::with_retention_days`] takes the caller's settings value.
    pub fn new(store: Arc<Mutex<LocalStore>>, workspace_id: impl Into<String>) -> Self {
        Self {
            store,
            workspace_id: workspace_id.into(),
            retention_gate: Arc::new(RetentionGate::new()),
            retention_days: RetentionWindow::new(DEFAULT_HISTORY_RETENTION_DAYS),
            prune_failure_report: None,
        }
    }

    /// The retention window the writer prunes by (REQ-HIST-04): the caller's
    /// `history_retention_days`, since this domain crate never reads `settings` itself.
    /// `0` disables pruning, as in `storage`'s `prune_older_than`. Later changes arrive
    /// through [`HistoryRecorder::set_retention_days`]. Additive, so
    /// [`HistoryRecorder::new`] keeps its signature.
    pub fn with_retention_days(self, retention_days: u32) -> Self {
        self.retention_days.set(retention_days);
        self
    }

    /// Update the retention window live (REQ-HIST-04): every clone of this recorder —
    /// including the drafts of sends already in flight — prunes by `retention_days` from
    /// now on, so a caller's settings change needs no restart (issue #196). `0` disables
    /// pruning, as in `storage`'s `prune_older_than`.
    pub fn set_retention_days(&self, retention_days: u32) {
        self.retention_days.set(retention_days);
    }

    /// Report a failed retention prune (REQ-HIST-04) through `report`, which receives
    /// the one fixed message `retention` folds the failure into — never the error
    /// itself, whose text can quote the rows it failed on (PRD §7). The caller that owes
    /// the user the message installs the report (AGENTS.md: errors reach the user); a
    /// recorder with no report logs the same fixed message instead.
    pub fn with_prune_failure_report(
        mut self,
        report: impl Fn(&'static str) + Send + Sync + 'static,
    ) -> Self {
        self.prune_failure_report = Some(Arc::new(report));
        self
    }

    /// Tell the user the retention prune failed (AGENTS.md: errors reach the user):
    /// through the report [`Self::with_prune_failure_report`] installs, which runs on
    /// the writer's background task and reaches the caller's own surface, or one fixed
    /// log line when there is none.
    fn report_prune_failure(&self, message: &'static str) {
        match &self.prune_failure_report {
            Some(report) => report(message),
            None => log::error!("{message}"),
        }
    }

    /// The record one send leaves behind (REQ-HIST-01 records every send). The request
    /// is redacted here with an empty [`SecretValues`]: the send's secrets are not known
    /// yet, and this draft is only ever written for a send that never produced any — a
    /// cancelled one.
    pub fn draft(&self, request: &HttpRequest) -> Option<Arc<HistoryDraft>> {
        self.draft_recorded_in(request, None)
    }

    /// [`HistoryRecorder::draft`] for a send from the folder whose installation key is
    /// `installation_key` (`None`: no folder): the row keeps it, and only a tab reopened
    /// in that same folder may resolve its variables (`pustman::scope_wiring`).
    pub fn draft_recorded_in(
        &self,
        request: &HttpRequest,
        installation_key: Option<String>,
    ) -> Option<Arc<HistoryDraft>> {
        self.draft_for_protocol(request, installation_key, PROTOCOL)
    }

    /// [`HistoryRecorder::draft_recorded_in`] for a send whose row names `protocol`: a
    /// GraphQL request goes out as HTTP but is recorded as `graphql` (REQ-GQL-01).
    pub(crate) fn draft_for_protocol(
        &self,
        request: &HttpRequest,
        installation_key: Option<String>,
        protocol: &'static str,
    ) -> Option<Arc<HistoryDraft>> {
        let (target, request_json) = redacted_parts(request, &SecretValues::new())?;
        Some(Arc::new(HistoryDraft {
            recorder: self.clone(),
            installation_key,
            protocol,
            method: request.method.as_str().to_owned(),
            target,
            request_json,
            created_at: OffsetDateTime::now_utc(),
            recorded: Mutex::new(false),
        }))
    }
}

impl fmt::Debug for HistoryRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryRecorder")
            .field("workspace_id", &self.workspace_id)
            .finish()
    }
}

/// The history one in-flight send leaves behind: the redacted draft a cancelled send
/// writes (the send's own work is dropped with it), plus the gate that keeps one send to
/// one row however its two would-be writers race. Shared between the send's work and
/// [`crate::HttpClient`]'s cancel hook.
///
/// `Debug` prints no request content: `target` and `request_json` are redacted but are
/// still request content, and request content never belongs in a log line.
pub struct HistoryDraft {
    recorder: HistoryRecorder,
    /// The installation key of the folder the send came from, stored beside the row.
    installation_key: Option<String>,
    /// The row's `protocol` column.
    protocol: &'static str,
    method: String,
    target: String,
    request_json: Value,
    created_at: OffsetDateTime,
    recorded: Mutex<bool>,
}

impl HistoryDraft {
    /// Row for a send that got its response. Runs inside the send's background work,
    /// before the outcome goes to the caller: the caller's task resolves only once the
    /// row is written. Never fails the send — a failed write logs one fixed message and
    /// the outcome travels on unchanged.
    pub fn record_response(
        &self,
        request: &HttpRequest,
        response: &Response,
        secrets_used: &SecretValues,
    ) {
        let Some((target, request_json)) = redacted_parts(request, secrets_used) else {
            log::error!("the history row could not be built");
            return;
        };
        let (window, body_size, may_continue) = body_window_and_size(&response.body, secrets_used);
        self.write(HistoryEntry {
            id: None,
            workspace_id: self.recorder.workspace_id.clone(),
            created_at: self.created_at,
            protocol: self.protocol.to_owned(),
            method: redact_text(&self.method, secrets_used),
            target,
            status: Some(i64::from(response.status)),
            duration_ms: response.timings.total.map(whole_millis),
            request_json,
            response_summary_json: Some(response_summary(response, secrets_used, body_size)),
            response_body_prefix: Some(redacted_body_prefix(&window, may_continue, secrets_used)),
        });
    }

    /// Row for a send that failed. `response_summary_json` names the error's variant as a
    /// fixed kind string — never the error text, which can quote the resolved URL or a
    /// path (PRD §7) — and no status, timing or body exists to store.
    pub fn record_error(
        &self,
        request: &HttpRequest,
        error: &PipelineError,
        secrets_used: &SecretValues,
    ) {
        let Some((target, request_json)) = redacted_parts(request, secrets_used) else {
            log::error!("the history row could not be built");
            return;
        };
        self.write(HistoryEntry {
            id: None,
            workspace_id: self.recorder.workspace_id.clone(),
            created_at: self.created_at,
            protocol: self.protocol.to_owned(),
            method: redact_text(&self.method, secrets_used),
            target,
            status: None,
            duration_ms: None,
            request_json,
            response_summary_json: Some(json!({ "error": error_kind(error) })),
            response_body_prefix: None,
        });
    }

    /// Row for a send that was cancelled before it finished. The send's task was dropped,
    /// so it can write no row of its own and this runs on a detached background task.
    pub fn record_cancelled(&self) {
        self.write(HistoryEntry {
            id: None,
            workspace_id: self.recorder.workspace_id.clone(),
            created_at: self.created_at,
            protocol: self.protocol.to_owned(),
            method: self.method.clone(),
            target: self.target.clone(),
            status: None,
            duration_ms: None,
            request_json: self.request_json.clone(),
            response_summary_json: Some(json!({ "cancelled": true })),
            response_body_prefix: None,
        });
    }

    /// Insert `entry`, at most once per send: the send's own work and the cancel hook can
    /// both race to record it, and REQ-HIST-01 is one row per send. The claim is taken
    /// before the insert, so a write is attempted at most once even when it fails. Once
    /// per app session the same write also prunes rows past the retention window
    /// (REQ-HIST-04). Runs on a background task (ADR-0002) and holds no lock across an
    /// await. Both failures — a poisoned lock and a failed insert — are one fixed
    /// message: the error behind them is dropped unread, because its text can quote the
    /// row it failed on. A failed prune behind the insert is reported to the user
    /// instead (AGENTS.md: errors reach the user — see [`Self::report_prune_failure`]).
    fn write(&self, entry: HistoryEntry) {
        let mut recorded = match self.recorded.lock() {
            Ok(recorded) => recorded,
            Err(_) => {
                log::error!("the history row could not be written");
                return;
            }
        };
        if *recorded {
            return;
        }
        *recorded = true;
        drop(recorded);
        match self.recorder.store.lock() {
            Ok(store) => {
                match store
                    .history()
                    .insert_recorded_in(&entry, self.installation_key.as_deref())
                {
                    Ok(_) => {}
                    Err(_) => log::error!("the history row could not be written"),
                }
                // Inside the same lock scope, after the insert: a failed prune never
                // blocks or undoes the row above.
                if let Some(message) = prune_once(
                    &self.recorder.retention_gate,
                    &store,
                    self.recorder.retention_days.days(),
                ) {
                    self.recorder.report_prune_failure(message);
                }
            }
            Err(_) => log::error!("the history row could not be written"),
        }
    }
}

impl fmt::Debug for HistoryDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryDraft")
            .field("method", &self.method)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// The row's `target` and `request_json`: the pre-resolution request as it was written,
/// redacted — the URL through [`redact_url`], the headers through [`redact_headers`],
/// the auth credential fields through [`mask_credential`], every other string and every
/// object key through [`redact_text`] (REQ-HIST-02/03). `None` when it cannot be built
/// at all.
fn redacted_parts(request: &HttpRequest, secrets: &SecretValues) -> Option<(String, Value)> {
    let target = redact_url(&request.url, secrets);
    let mut document = serde_json::to_value(request).ok()?;
    let fields = document.as_object_mut()?;
    redact_object(fields, secrets, |key, field| match key {
        "url" => {
            if let Some(url) = field.as_str() {
                let masked = redact_url(url, secrets);
                *field = Value::String(masked);
            } else {
                redact_leaves(field, secrets);
            }
        }
        "auth" => redact_auth(&request.auth, field, secrets),
        "headers" => redact_header_table(field, secrets),
        _ => redact_leaves(field, secrets),
    });
    Some((target, document))
}

/// Rebuild one JSON object with every key masked through [`redact_text`] and every value
/// decided by `redact_value` (matched on the ORIGINAL key). Keys hold request content
/// too: `extra` maps are flattened into their host objects, so a secret used as a JSON
/// key is masked like any other string. Two keys can redact to one mask and collapse
/// into one entry — the row is a summary, and masking comes first.
fn redact_object(
    fields: &mut serde_json::Map<String, Value>,
    secrets: &SecretValues,
    mut redact_value: impl FnMut(&str, &mut Value),
) {
    let mut redacted = serde_json::Map::with_capacity(fields.len());
    for (key, mut field) in std::mem::take(fields) {
        redact_value(&key, &mut field);
        redacted.insert(redact_text(&key, secrets), field);
    }
    *fields = redacted;
}

/// Redact one JSON node: a `headers` table row by row through [`redact_headers`], every
/// string leaf — and every object key — through [`redact_text`] with the send's secrets.
/// Nested `headers` tables — an imported example's, a kept-verbatim field's — get the
/// header rules too: a cookie is a credential wherever it is kept (REQ-HIST-03).
fn redact_leaves(node: &mut Value, secrets: &SecretValues) {
    match node {
        Value::String(text) => *text = redact_text(text, secrets),
        Value::Array(items) => {
            for item in items {
                redact_leaves(item, secrets);
            }
        }
        Value::Object(fields) => redact_object(fields, secrets, |key, field| {
            if key == "headers" {
                redact_header_table(field, secrets);
            } else {
                redact_leaves(field, secrets);
            }
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// One `headers` table: each row's name/value pair through [`redact_headers`], so a
/// credential header is masked whole whatever it carries (REQ-HIST-03), and the row's
/// other fields — descriptions, imported extras — as ordinary leaves.
fn redact_header_table(table: &mut Value, secrets: &SecretValues) {
    let Some(rows) = table.as_array_mut() else {
        redact_leaves(table, secrets);
        return;
    };
    for row in rows {
        let Some(fields) = row.as_object_mut() else {
            redact_leaves(row, secrets);
            continue;
        };
        let name = fields.get("key").and_then(Value::as_str).map(str::to_owned);
        let Some(name) = name else {
            redact_leaves(row, secrets);
            continue;
        };
        let value = fields
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let mut redacted = redact_headers(&[(name, value)], secrets);
        if let Some((name, value)) = redacted.pop() {
            fields.insert("key".to_owned(), Value::String(name));
            if fields.contains_key("value") {
                fields.insert("value".to_owned(), Value::String(value));
            }
        }
        redact_object(fields, secrets, |key, field| {
            if key != "key" && key != "value" {
                redact_leaves(field, secrets);
            }
        });
    }
}

/// The request's `auth` node. The credential fields — `Basic`'s username and password,
/// `Bearer`'s token, `ApiKey`'s value — and the whole of an `Unsupported` auth are
/// credentials whatever they hold: [`mask_credential`] decides each one. Everything else
/// in the node is an ordinary leaf.
fn redact_auth(kind: &Auth, node: &mut Value, secrets: &SecretValues) {
    let credential_fields: &[&str] = match kind {
        Auth::Inherit | Auth::None => &[],
        Auth::Basic { .. } => &["username", "password"],
        Auth::Bearer { .. } => &["token"],
        Auth::ApiKey { .. } => &["value"],
        Auth::Unsupported(_) => {
            mask_credential(node);
            return;
        }
    };
    let Some(fields) = node.as_object_mut() else {
        redact_leaves(node, secrets);
        return;
    };
    redact_object(fields, secrets, |key, field| {
        if credential_fields.contains(&key) {
            mask_credential(field);
        } else {
            redact_leaves(field, secrets);
        }
    });
}

/// One auth credential: a value that is exactly one `{{name}}` reference stays as written
/// — it restores on REQ-HIST-02 and its resolved value is masked through the send's
/// secrets anyway — and any other value becomes the shared mask (REQ-HIST-03).
fn mask_credential(node: &mut Value) {
    if let Some(text) = node.as_str()
        && is_one_reference(text)
    {
        return;
    }
    *node = Value::String(shared_mask());
}

/// Whether `value` is exactly one `{{name}}` reference and nothing else. Parsed through
/// `variables`' own placeholder grammar, never a second one.
fn is_one_reference(value: &str) -> bool {
    let found = spans(value, &ScopeStack::new());
    matches!(
        found.as_slice(),
        [span] if span.range.start == 0 && span.range.end == value.len()
    )
}

/// The mask an auth credential becomes: the glyph [`redact_text`] replaces a secret with.
/// `api_model` keeps its mask private and a second literal here could drift from the
/// masking everywhere else (REQ-NFR-04 is one shared `redact`), so it is taken from the
/// shared redaction itself — masking a value registered as a secret of itself yields
/// exactly the glyph.
fn shared_mask() -> String {
    let probe = "pustman-shared-mask-probe";
    redact_text(probe, &SecretValues::from_values([probe]))
}

/// The stored response summary (REQ-HIST-02): the status, the timings, the body's total
/// size and the response headers through [`redact_headers`], so `Set-Cookie` is masked
/// whole like any credential (REQ-HIST-03).
fn response_summary(response: &Response, secrets: &SecretValues, body_size: u64) -> Value {
    let headers: Vec<(String, String)> = response
        .headers
        .iter()
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect();
    let headers: Vec<Value> = redact_headers(&headers, secrets)
        .into_iter()
        .map(|(name, value)| json!({ "name": name, "value": value }))
        .collect();
    json!({
        "status": response.status,
        "timings": {
            "prepare_ms": millis_value(response.timings.prepare),
            "dns_ms": millis_value(response.timings.dns),
            "connect_ms": millis_value(response.timings.connect),
            "tls_ms": millis_value(response.timings.tls),
            "first_byte_ms": millis_value(response.timings.first_byte),
            "download_ms": millis_value(response.timings.download),
            "total_ms": millis_value(response.timings.total),
        },
        "body_size": body_size,
        "headers": headers,
    })
}

/// A measured phase as whole milliseconds, or `null` when the transport could not
/// measure it — a phase is never estimated, as [`crate::Timings`] requires.
fn millis_value(duration: Option<Duration>) -> Value {
    duration.map_or(Value::Null, |duration| json!(whole_millis(duration)))
}

/// A duration as whole milliseconds, clamped where `i64` cannot hold it.
fn whole_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// The body window a history row reads — at most [`HISTORY_BODY_LIMIT`] bytes plus the
/// longest secret (REQ-HIST-02 caps what is STORED; the read goes further so redaction
/// sees whole every secret the stored cut could split) — and the body's total size. A
/// streamed body tops its in-memory prefix up from the file, which is file IO and
/// therefore only ever runs inside the recording background task (ADR-0002). The flag
/// says whether content may continue past the window; it is stated rather than derived
/// from the size because a file whose size cannot be read reports only its prefix's
/// length, and an under-reported size must never make a cut window look complete.
fn body_window_and_size(body: &ResponseBody, secrets: &SecretValues) -> (Vec<u8>, u64, bool) {
    let window_limit = HISTORY_BODY_LIMIT + secrets.max_len();
    let (window, size, size_known) = match body {
        ResponseBody::Bytes(bytes) => (
            bytes
                .iter()
                .copied()
                .take(window_limit)
                .collect::<Vec<u8>>(),
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            true,
        ),
        // The file holds the whole body, the prefix its first bytes.
        ResponseBody::File { path, prefix } => {
            let offset = u64::try_from(prefix.len()).unwrap_or(u64::MAX);
            let window = topped_up(prefix, path, offset, window_limit);
            match std::fs::metadata(path) {
                Ok(metadata) => (window, metadata.len(), true),
                Err(_) => (window, offset, false),
            }
        }
        // The file holds the tail that did not fit the in-memory prefix.
        ResponseBody::Spilled {
            prefix,
            file,
            total_len,
        } => (
            topped_up(prefix, file.path(), 0, window_limit),
            *total_len,
            true,
        ),
    };
    let may_continue = !size_known || u64::try_from(window.len()).unwrap_or(u64::MAX) < size;
    (window, size, may_continue)
}

/// `prefix` filled up to `window` bytes with the bytes of `file` that follow
/// `file_offset`. A read that fails leaves the in-memory prefix: the row is a summary,
/// so losing tail bytes costs detail and never correctness, and the failing path is one
/// a user cannot act on.
fn topped_up(prefix: &[u8], file: &Path, file_offset: u64, window: usize) -> Vec<u8> {
    let mut bytes: Vec<u8> = prefix.iter().copied().take(window).collect();
    let missing = window.saturating_sub(bytes.len());
    if missing == 0 {
        return bytes;
    }
    let Ok(mut opened) = std::fs::File::open(file) else {
        return bytes;
    };
    let mut tail = Vec::new();
    let read = opened.seek(SeekFrom::Start(file_offset)).and_then(|_| {
        opened
            .by_ref()
            .take(u64::try_from(missing).unwrap_or(u64::MAX))
            .read_to_end(&mut tail)
    });
    if read.is_ok() {
        bytes.extend_from_slice(&tail);
    }
    bytes
}

/// The stored body prefix. Only a window cut short by its bound can end mid-occurrence,
/// so only then does it lose the tail a secret occurrence could straddle the end of
/// ([`SecretValues::straddle_free_prefix`] — the rest of that occurrence lies beyond the
/// window and must never be half kept) and any incomplete UTF-8 sequence at its end (a
/// cut mid-character would turn into U+FFFD where the secret's own bytes are expected,
/// and the tail would no longer look like the secret prefix it is). A window that holds
/// the whole body keeps its tail: ending like a secret prefix is content, and the
/// occurrence either lies whole inside the window — [`redact_text`] masks it — or does
/// not exist. Then the text runs through the shared [`redact_text`] with the send's
/// secrets, then is cut to [`HISTORY_BODY_LIMIT`] bytes — redaction can grow text and the
/// mask is multi-byte, so the cut lands on a char boundary. The order matters: cutting
/// the raw bytes first would store the unmasked start of a secret that straddles the cut.
fn redacted_body_prefix(
    window: &[u8],
    content_past_window: bool,
    secrets: &SecretValues,
) -> Vec<u8> {
    let window = if content_past_window {
        without_incomplete_utf8_tail(window)
    } else {
        window
    };
    let lossy = String::from_utf8_lossy(window);
    let kept = if content_past_window {
        secrets.straddle_free_prefix(&lossy)
    } else {
        lossy.as_ref()
    };
    let mut redacted = redact_text(kept, secrets);
    if redacted.len() > HISTORY_BODY_LIMIT {
        let mut end = HISTORY_BODY_LIMIT;
        while !redacted.is_char_boundary(end) {
            end -= 1;
        }
        redacted.truncate(end);
    }
    redacted.into_bytes()
}

/// `bytes` without an incomplete UTF-8 sequence at its end. A window cut short can stop
/// between the bytes of one character; lossy conversion would rewrite that dangling lead
/// byte to U+FFFD where a secret's own bytes are expected. Invalid bytes earlier in the
/// window are the body's own bytes and stay: only the tail the window cut is dropped.
fn without_incomplete_utf8_tail(bytes: &[u8]) -> &[u8] {
    let mut scanned = 0;
    // Each round advances `scanned` by at most the length of the slice it just read, so
    // `scanned` never passes `bytes.len()` and every round ends in a return: the empty
    // tail a lookup past the end would be is valid UTF-8, which returns `bytes` too.
    loop {
        let remaining = bytes.get(scanned..).unwrap_or_default();
        let error = match std::str::from_utf8(remaining) {
            Ok(_) => return bytes,
            Err(error) => error,
        };
        match error.error_len() {
            // An incomplete sequence at the end: everything before it stays.
            None => {
                return bytes
                    .get(..scanned + error.valid_up_to())
                    .unwrap_or_default();
            }
            // A definite invalid subpart stays lossy; scan what follows it.
            Some(length) => scanned += error.valid_up_to() + length,
        }
    }
}

/// The error's variant as a fixed kind string. Never the error's text: `Display`,
/// `Debug` and `user_message` can all quote the resolved URL, a host or a path (PRD §7),
/// so the row names the kind and nothing else.
fn error_kind(error: &PipelineError) -> &'static str {
    match error {
        PipelineError::Resolve(_) => "resolve",
        PipelineError::Transport(error) => match error {
            TransportError::InvalidUrl { .. } => "invalid_url",
            TransportError::Dns { .. } => "dns",
            TransportError::Connect { .. } => "connect",
            TransportError::Tls { .. } => "tls",
            TransportError::Timeout { .. } => "timeout",
            TransportError::TooManyRedirects { .. } => "too_many_redirects",
            TransportError::Proxy { .. } => "proxy",
            TransportError::Protocol { .. } => "protocol",
            TransportError::Io { .. } => "io",
            TransportError::FileNotFound { .. } => "file_not_found",
            TransportError::UnresolvedSecret { .. } => "unresolved_secret",
            TransportError::Cancelled => "cancelled",
        },
    }
}
