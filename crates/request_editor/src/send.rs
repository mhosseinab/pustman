//! Sending the request this tab edits, and cancelling it (PRD REQ-HTTP-12, REQ-RESP-01).
//!
//! `request::Send` (Ctrl/Cmd-Enter) sends through [`HttpClient::send_request`] and
//! stores the task in `RequestEditor::pending_send`: replacing it cancels the request
//! before it, exactly as AGENTS.md prescribes for a dropped `Task`. `request::Cancel`
//! (Escape) drops the task and aborts the connection.
//!
//! The send is the full pipeline (S33e): the draft resolves against the editor's own
//! scopes — `RequestEditor`'s `scopes`, the very stack the fields highlight and hover
//! against — the Auth tab's auth applies, referenced secrets come from the keychain
//! through the editor's `variable_hover` `SecretSources`, and the client records
//! history. One variable set feeds the highlighting, the pre-send check and the request
//! on the wire. Every finished send records exactly one Console outcome and every
//! failed send one Console Error entry (REQ-CON-01, REQ-CON-04); a cancelled send
//! records none here, because `console::watch` in the binary already records it
//! (REQ-HTTP-12) — never a second write. What the send produced lands in the
//! [`response_viewer::ResponseView`] embedded below the request panes (S29e) — its own
//! status line and body — while the editor keeps the [`StatusLine`] facts computed from
//! the response so `RequestEditor::status_line()` still reports them. A failure lands
//! inline under the URL bar as the transport's own [`TransportError::user_message`] and
//! [`TransportError::suggested_fix`], verbatim, never re-worded here (AGENTS.md — errors
//! reach the user), and in the embedded viewer's error panel — one wording, shown both
//! where the send happened and where responses live. A TLS failure's inline display
//! additionally offers "Disable verification for this request" (REQ-HTTP-10): it pins
//! `verify_tls` off on this request's draft and sends again, with the warning badge
//! beside Send for as long as the override is on. No global setting is ever changed
//! from here.
//!
//! `Decision:` the editor sends through the [`RequestClient`] global instead of holding
//! a client of its own: `new_request`'s app-level handler constructs `RequestEditor` and
//! has no transport to build a client from, and this step's Edits list keeps
//! `new_request.rs` untouched. The app installs the client (S26w builds it on
//! `ReqwestTransport`); tests install one backed by `http_engine::test_support`'s
//! `FakeTransport`. `request_editor::init` cannot install one either: the transport is
//! an app-level wiring decision.

use std::collections::BTreeMap;
use std::time::Duration;

use api_model::HttpRequest;
use console::{ConsoleStore, record_error, record_outcome, record_warnings};
use gpui::{App, Context, Entity, Global, Role, Window};
use http_engine::{
    HttpClient, PipelineError, Response, ResponseBody, SendInput, SendOutcome, TransportError,
    Warning, resolve_request,
};
use pustman_actions::request;
use ui::prelude::*;
use variables::{DynamicValues, ScopeStack};

use crate::RequestEditor;
use crate::settings_tab::SettingChange;
use crate::unresolved_warning::{UnresolvedWarning, unresolved_names};

/// Test hook: the button that sends the request, when nothing is in flight.
pub const SEND_BUTTON_SELECTOR: &str = "send-button";

/// Test hook: the same button while a request is in flight, when it reads "Cancel".
pub const CANCEL_BUTTON_SELECTOR: &str = "cancel-button";

/// Test hook: the status line — now the embedded [`response_viewer::ResponseView`]'s
/// own (S29e moved the rendering out of the editor). Re-exported so `send`'s callers
/// and tests keep exactly one key for the one status line on screen.
pub use response_viewer::STATUS_LINE_SELECTOR;

/// Test hook: the inline failure under the URL bar.
pub const INLINE_ERROR_SELECTOR: &str = "inline-error";

/// Test hook: the warning badge beside Send while TLS verification is off for this
/// request.
pub const TLS_BADGE_SELECTOR: &str = "tls-verification-badge";

/// Test hook: the "Disable verification for this request" offer on a TLS failure.
pub const TLS_OFFER_SELECTOR: &str = "disable-tls-verification";

/// The HTTP client every request tab sends through. The app installs it once at
/// startup; a test installs one backed by `FakeTransport`.
pub struct RequestClient(pub Entity<HttpClient>);

impl Global for RequestClient {}

/// Install the client the request editor sends through.
pub fn set_http_client(client: Entity<HttpClient>, cx: &mut App) {
    cx.set_global(RequestClient(client));
}

/// Why a send failed and what to do about it, shown inline under the URL bar — where
/// the send happened. Both texts come from the transport, untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineError {
    /// [`TransportError::user_message`]: what happened.
    pub message: String,
    /// [`TransportError::suggested_fix`]: what to do about it.
    pub fix: String,
}

impl InlineError {
    /// The inline form of a transport failure: both texts, verbatim.
    fn from_transport(error: &TransportError) -> Self {
        Self {
            message: error.user_message(),
            fix: error.suggested_fix().to_string(),
        }
    }
}

/// The facts of the last response, as `RequestEditor::status_line()` reports them: the
/// code, the reason phrase, the total time and the body size (REQ-RESP-01). The
/// rendering lives in `response_viewer`'s own status line now (S29e); these facts stay
/// so the accessor keeps working. A fact the transport did not measure is [`None`] and
/// renders as "—", never estimated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub status: u16,
    pub reason: String,
    pub total: Option<Duration>,
    pub size: Option<u64>,
}

impl StatusLine {
    fn from_response(response: &Response) -> Self {
        Self {
            status: response.status,
            reason: response.reason.clone(),
            total: response.timings.total,
            size: body_size(&response.body),
        }
    }
}

/// The bytes a body is known to carry. A streamed file's length is not observable here,
/// so it stays [`None`] rather than being guessed.
fn body_size(body: &ResponseBody) -> Option<u64> {
    match body {
        ResponseBody::Bytes(bytes) => Some(bytes.len() as u64),
        ResponseBody::File { .. } => None,
        ResponseBody::Spilled { total_len, .. } => Some(*total_len),
    }
}

impl RequestEditor {
    /// Whether a send is still in flight — what the Send button renders "Cancel" from.
    pub fn is_sending(&self) -> bool {
        self.pending_send.is_some()
    }

    /// Why the last send failed, if it did.
    pub fn inline_error(&self) -> Option<&InlineError> {
        self.inline_error.as_ref()
    }

    /// The failure on display was earned by the request as it was: an edit makes it
    /// stale (#206), so the edit clears it — and the TLS offer that belongs to that
    /// very failure with it. The response pane keeps the failure as the record of the
    /// last send; the next send shows whatever its own request earns.
    pub(super) fn clear_stale_error(&mut self) {
        self.inline_error = None;
        self.tls_offer_error = None;
    }

    /// The facts of the last response, if a send has completed.
    pub fn status_line(&self) -> Option<&StatusLine> {
        self.status_line.as_ref()
    }

    /// The label on the send button: "Cancel" while a request is in flight.
    pub fn send_button_label(&self) -> &'static str {
        if self.is_sending() { "Cancel" } else { "Send" }
    }

    /// `request::Send` (Ctrl/Cmd-Enter) — the keyboard's side of the Send button.
    pub(super) fn handle_send(
        &mut self,
        _: &request::Send,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_send(cx);
    }

    /// `request::Cancel` (Escape) — cancel the send in flight. With nothing to cancel it
    /// keeps bubbling: the keystroke is not this editor's to consume.
    pub(super) fn handle_cancel(
        &mut self,
        _: &request::Cancel,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_sending() {
            cx.propagate();
            return;
        }
        self.cancel_send(cx);
    }

    /// Send the draft. The send task lands in `pending_send`, so a send while one is
    /// still in flight replaces — and so cancels — the request before it. A draft that
    /// would use unresolved variables is held back instead: the warning bar lists them
    /// and "Send anyway" is the only way past it (REQ-VAR-07).
    pub fn start_send(&mut self, cx: &mut Context<Self>) {
        self.begin_send(cx, false);
    }

    /// The warning bar's "Send anyway" (REQ-VAR-07): send with the unresolved names left
    /// literal — the bar's explicit choice, never a keystroke. The bar goes with the
    /// send, and the warning travels with the outcome into the Console.
    pub(super) fn send_anyway(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.unresolved_warning = None;
        self.begin_send(cx, true);
        // The button this click landed on is gone with the bar; a keyboard user's focus
        // must not disappear with it.
        window.focus(&self.focus_handle, cx);
    }

    /// Send the draft; `send_unresolved` names the one caller that means "anyway".
    /// The pre-send check and the send resolve against the same `self.scopes`, so the
    /// bar only ever holds names really unresolved against the editor's scopes, and
    /// those names stay literal and go out as they are (REQ-VAR-07).
    fn begin_send(&mut self, cx: &mut Context<Self>, send_unresolved: bool) {
        let Some(client) = cx
            .try_global::<RequestClient>()
            .map(|client| client.0.clone())
        else {
            self.tls_offer_error = None;
            self.inline_error = Some(InlineError {
                message: String::from("Pustman's HTTP client is not available."),
                fix: String::from("Restart the app — the request was not sent."),
            });
            cx.notify();
            return;
        };
        // What the send carries is also what a failure's Console entry names.
        let sent = self.draft.clone();
        let warnings = match resolve_request(&self.draft, &self.scopes, &mut DynamicValues::new()) {
            Ok((_, warnings)) => warnings,
            Err(error) => {
                self.fail_send(&sent, &error, &[], cx);
                return;
            }
        };
        if !send_unresolved && let Some(names) = unresolved_names(&warnings) {
            self.unresolved_warning = Some(UnresolvedWarning::new(names.to_vec()));
            cx.notify();
            return;
        }
        self.unresolved_warning = None;
        // The response region starts over with the send: its old facts and panes must
        // never show beside what this send produces (S33n).
        self.clear_previous_response(cx);
        let input = SendInput {
            request: sent.clone(),
            // The Auth tab's parent chain, nearest first — `prepare`'s own contract —
            // so the wire carries exactly the effective auth the tab previews. It is
            // empty in M1: nothing calls `AuthTab::set_parents` outside tests yet.
            ancestors: self
                .auth_tab
                .read(cx)
                .parents()
                .iter()
                .map(|(_, auth)| auth.clone())
                .collect(),
            scopes: scopes_for_send(&self.scopes),
            secrets: self.secret_sources.store.clone(),
            secret_key: self.secret_sources.key_of.clone(),
        };
        let send_task = client.update(cx, |client, cx| client.send_request(input, cx));
        self.inline_error = None;
        self.tls_offer_error = None;
        self.pending_send = Some(cx.spawn(async move |this, cx| {
            let result = send_task.await;
            let landed = this.update(cx, |editor, cx| {
                editor.finish_send(result, sent, warnings, cx)
            });
            if landed.is_err() {
                // Handled: the tab closed while the request was in flight; there is no
                // status line left to write to.
            }
        }));
        cx.notify();
    }

    /// Cancel the send in flight: dropping `pending_send` discards its result and
    /// [`HttpClient::cancel`] drops the task that holds the connection (REQ-HTTP-12).
    /// A user cancel is not a failure — no error is shown — but it does show its own
    /// state where responses live (S33n): the previous response must not survive
    /// beside it.
    pub fn cancel_send(&mut self, cx: &mut Context<Self>) {
        self.pending_send = None;
        self.show_cancelled_state(cx);
        if let Some(client) = cx
            .try_global::<RequestClient>()
            .map(|client| client.0.clone())
        {
            client.update(cx, |client, cx| client.cancel(cx));
        }
        cx.notify();
    }

    /// The cancelled state in the response region (the response states' "Client
    /// canceled"): the pane says the client stopped waiting — info level, not a
    /// failure (`response_viewer`'s error panel maps [`TransportError::Cancelled`] to
    /// severity info) — with the previous response gone (S33n).
    fn show_cancelled_state(&mut self, cx: &mut Context<Self>) {
        self.clear_previous_response(cx);
        self.response_view
            .update(cx, |view, cx| view.set_error(TransportError::Cancelled, cx));
    }

    /// Start the response region over: the previous response's facts
    /// (`RequestEditor::status_line`) and the panes below it must never survive into
    /// what the next outcome shows — a failed or cancelled send may not sit beside the
    /// old 200 (S33n). [`response_viewer::ResponseView::clear`] is the clear: it resets
    /// what the region shows while keeping the entity — and its in-flight save/copy
    /// tasks — alive (#264), and the error state replaces the strip only when the view
    /// holds no response of its own.
    fn clear_previous_response(&mut self, cx: &mut Context<Self>) {
        self.status_line = None;
        self.response_view.update(cx, |view, cx| view.clear(cx));
    }

    /// A send that failed before or at the transport: one Console Error entry
    /// (REQ-CON-01/04) and the failure shown inline, exactly as a transport failure is
    /// shown. This is the one place a failed send lands and the one place its Console
    /// entry is written — [`console::feed`]'s [`record_error`] does the redacting — and
    /// the warnings the send carried are recorded with it (REQ-VAR-07): a "Send anyway"
    /// failure names its unresolved variables exactly as a finished send's outcome does.
    /// `sent` is the request as the send had it, for the entry's summary line.
    ///
    /// `Decision:` the write keeps the [`ConsoleStore`]-global guard of the warned-send
    /// write it replaces: a host that has not installed the console store (an editor
    /// test without `console::init`) records nothing rather than crashing the send path.
    fn fail_send(
        &mut self,
        sent: &HttpRequest,
        error: &PipelineError,
        warnings: &[Warning],
        cx: &mut Context<Self>,
    ) {
        self.pending_send = None;
        self.clear_previous_response(cx);
        if cx.try_global::<ConsoleStore>().is_some() {
            record_error(Some(sent), error, cx);
            record_warnings(Some(sent), warnings, cx);
        }
        match error {
            PipelineError::Transport(error) => self.show_transport_error(error, cx),
            // `ResolveError` carries no `suggested_fix` of its own (its `Display` names
            // the variables involved and nothing else), so only the message is its text.
            PipelineError::Resolve(error) => {
                self.tls_offer_error = None;
                self.inline_error = Some(InlineError {
                    message: error.to_string(),
                    fix: String::from("Check the variables this request references."),
                });
                cx.notify();
            }
        }
    }

    /// Show `error` inline where the send happened: both texts verbatim (AGENTS.md —
    /// errors reach the user). The same failure also lands in the embedded
    /// [`response_viewer::ResponseView`]'s error panel (S29e), so it is visible both
    /// where the send happened and where responses live — one wording, two places.
    /// A TLS failure keeps its offer form in `tls_offer_error`, so "Disable
    /// verification for this request" renders for it — and, because the shown error
    /// must be that very failure, never for another error that replaces `inline_error`
    /// outside a send.
    fn show_transport_error(&mut self, error: &TransportError, cx: &mut Context<Self>) {
        let inline = InlineError::from_transport(error);
        self.tls_offer_error = matches!(error, TransportError::Tls { .. }).then(|| inline.clone());
        self.inline_error = Some(inline);
        self.response_view
            .update(cx, |view, cx| view.set_error(error.clone(), cx));
        cx.notify();
    }

    /// The send ended: the response goes to the embedded
    /// [`response_viewer::ResponseView`] (and its facts to `status_line()`), or the
    /// failure shows inline and in the viewer's error panel through [`Self::fail_send`].
    /// Either way this is the send's one Console write (REQ-CON-01): a finished send's
    /// [`record_outcome`] carries its warnings into the Console with it (REQ-VAR-07),
    /// and a failed send's entry comes from [`Self::fail_send`], which records the
    /// warnings `begin_send` resolved the request with alongside the error. Those
    /// warnings are the pre-send check's own (REQ-VAR-07): empty unless the user sent
    /// past the warning bar.
    fn finish_send(
        &mut self,
        result: Result<SendOutcome, PipelineError>,
        sent: HttpRequest,
        warnings: Vec<Warning>,
        cx: &mut Context<Self>,
    ) {
        self.pending_send = None;
        match result {
            Ok(outcome) => {
                self.status_line = Some(StatusLine::from_response(&outcome.response));
                self.inline_error = None;
                self.tls_offer_error = None;
                if cx.try_global::<ConsoleStore>().is_some() {
                    record_outcome(&outcome, cx);
                }
                self.response_view
                    .update(cx, |view, cx| view.set_response(outcome.response, cx));
            }
            // A cancelled or replaced send is not a failure to report (REQ-HTTP-12) and
            // records nothing here: `console::watch` in the binary already records it.
            // It still shows its own state, so the previous response never sits beside
            // it (S33n).
            Err(PipelineError::Transport(TransportError::Cancelled)) => {
                self.show_cancelled_state(cx)
            }
            Err(error) => self.fail_send(&sent, &error, &warnings, cx),
        }
        cx.notify();
    }

    /// The end of the URL row: a "Sending…" note, the TLS badge and the Send/Cancel
    /// button. The response's own status line is the embedded
    /// [`response_viewer::ResponseView`]'s (S29e).
    pub(super) fn render_send_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_none()
            .gap(DynamicSpacing::Base03.rems(cx))
            .items_center()
            .children(self.render_sending(cx))
            .children(self.render_tls_badge(cx))
            .child(self.render_send_button(cx))
            .into_any_element()
    }

    /// The inline failure under the URL row, where the send happened.
    pub(super) fn render_send_error(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.inline_error
            .clone()
            .map(|error| self.render_inline_error(&error, cx))
    }

    /// "Sending…" beside the button while a request is in flight — the one piece of
    /// send state this row still shows (the response's facts are the viewer's status
    /// line now, S29e).
    fn render_sending(&self, _cx: &App) -> Option<AnyElement> {
        self.is_sending().then(|| {
            Label::new(SharedString::new_static("Sending…"))
                .size(LabelSize::Small)
                .color(Color::Muted)
                .into_any_element()
        })
    }

    /// The failure under the URL bar: what happened, what to do about it, and — for
    /// the TLS failure the offer belongs to — the way past it (REQ-HTTP-10).
    fn render_inline_error(&self, error: &InlineError, cx: &Context<Self>) -> AnyElement {
        let offers_tls_disable = self.tls_offer_error.as_ref() == Some(error);
        v_flex()
            .debug_selector(|| INLINE_ERROR_SELECTOR.to_string())
            .gap(DynamicSpacing::Base01.rems(cx))
            .child(
                Label::new(SharedString::from(error.message.clone()))
                    .size(LabelSize::Small)
                    .color(Color::Error),
            )
            .child(
                Label::new(SharedString::from(error.fix.clone()))
                    .size(LabelSize::Small)
                    .color(Color::Hint),
            )
            .children(offers_tls_disable.then(|| self.render_tls_offer(cx)))
            .into_any_element()
    }

    /// The way past a self-signed or otherwise untrusted certificate: pin verification
    /// off on THIS request and send it again. A [`Button`], so it is a tab stop Enter
    /// and Space activate.
    fn render_tls_offer(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .debug_selector(|| TLS_OFFER_SELECTOR.to_string())
            .child(
                Button::new(
                    "disable-tls-verification",
                    SharedString::new_static("Disable verification for this request"),
                )
                .size(ButtonSize::Medium)
                .style(ButtonStyle::Outlined)
                .tooltip(Tooltip::text(SharedString::new_static(
                    "Turn TLS verification off for this request only, then send it again",
                )))
                .on_click(cx.listener(|this, _event, window, cx| {
                    this.disable_verification_and_resend(window, cx)
                })),
            )
            .into_any_element()
    }

    /// The offer on a TLS failure (REQ-HTTP-10, the HTTP acceptance criterion about
    /// self-signed servers): pin `verify_tls` off on this draft — the Settings tab
    /// reflects it and the badge shows — then call the existing send path again. The
    /// draft is written first so the send below carries it: the tab's change event
    /// would only reach the draft after this update, too late here. A global setting
    /// is never changed.
    ///
    /// Focus goes back to the editor, because the offer's own error is about to be
    /// replaced by the re-send's outcome: the button this click landed on disappears
    /// with it, and a keyboard user's focus must not disappear too.
    pub(super) fn disable_verification_and_resend(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_setting_change(SettingChange::VerifyTls(Some(false)), cx);
        self.settings_tab
            .update(cx, |tab, cx| tab.reseed(&self.draft, cx));
        window.focus(&self.focus_handle, cx);
        self.start_send(cx);
    }

    /// The persistent warning beside Send while TLS verification is off for this
    /// request (REQ-HTTP-10) — a badge, not a toast: it stays up until the request
    /// verifies again, and the text says what the color repeats.
    fn render_tls_badge(&self, _cx: &App) -> Option<AnyElement> {
        let verification_off = self
            .draft
            .settings
            .as_ref()
            .and_then(|settings| settings.verify_tls)
            == Some(false);
        if !verification_off {
            return None;
        }
        Some(
            h_flex()
                .id(ElementId::Name(SharedString::new_static(
                    "tls-verification-badge",
                )))
                .debug_selector(|| TLS_BADGE_SELECTOR.to_string())
                .role(Role::Status)
                .aria_label(SharedString::new_static(
                    "Warning: TLS certificate verification is disabled for this request",
                ))
                .tooltip(
                    Tooltip::text(SharedString::new_static(
                        "TLS certificate verification is off for this request only — the \
                         global default is unchanged",
                    ))
                    .builder(),
                )
                .child(
                    Badge::new(SharedString::new_static("TLS verification off"))
                        .severity(Severity::Warning)
                        .icon(IconName::TriangleAlert),
                )
                .into_any_element(),
        )
    }

    /// The Send button — "Cancel" while a request is in flight, with the keystroke that
    /// does the same thing spelled out in its tooltip.
    fn render_send_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let in_flight = self.is_sending();
        let label = self.send_button_label();
        let button = Button::new("send-request", SharedString::new_static(label))
            .style(if in_flight {
                ButtonStyle::Outlined
            } else {
                ButtonStyle::Filled
            })
            .size(ButtonSize::Large)
            // The keystroke lives in the tooltip, not on the button: a key chip on the
            // filled accent background is unreadable.
            .tooltip(if in_flight {
                Tooltip::for_action("Cancel the request in flight", &request::Cancel)
            } else {
                Tooltip::for_action("Send this request", &request::Send)
            })
            .on_click(cx.listener(|this, _event, _window, cx| {
                if this.is_sending() {
                    this.cancel_send(cx);
                } else {
                    this.start_send(cx);
                }
            }));
        // The selector is the state the button renders in, so a test sees which of the
        // two rendered.
        div()
            .debug_selector(move || {
                if in_flight {
                    CANCEL_BUTTON_SELECTOR
                } else {
                    SEND_BUTTON_SELECTOR
                }
                .to_string()
            })
            .child(button)
            .into_any_element()
    }
}

/// The editor's scopes as the owned [`ScopeStack`] one [`SendInput`] takes: the send
/// runs on the background executor and takes the stack by value, while the editor keeps
/// sharing its own (an `Arc`) with every field. The rebuild is the shape
/// `http_engine::prepare` itself uses (`scopes_with_secrets`): copy the scopes and, with
/// them, the local values — which resolution reads only by variable key
/// (`ScopeStack::find`), so nothing a send can observe is lost (S33e).
fn scopes_for_send(scopes: &ScopeStack) -> ScopeStack {
    let mut local_values = BTreeMap::new();
    for scope in scopes.scopes() {
        for variable in &scope.variables {
            if let Some(value) = scopes.local_value(&variable.key) {
                local_values.insert(variable.key.clone(), value.to_string());
            }
        }
    }
    let mut owned = ScopeStack::with_local_values(local_values);
    for scope in scopes.scopes() {
        owned.push(scope.kind, scope.variables.clone());
    }
    owned
}

#[cfg(test)]
#[path = "send_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "send_pipeline_tests.rs"]
mod pipeline_tests;

#[cfg(test)]
#[path = "send_row_height_tests.rs"]
mod row_height_tests;

#[cfg(test)]
#[path = "send_warned_failure_tests.rs"]
mod warned_failure_tests;
