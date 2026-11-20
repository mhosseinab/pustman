//! Sending the request this tab edits, and cancelling it (PRD REQ-HTTP-12, REQ-RESP-01).
//!
//! `request::Send` (Ctrl/Cmd-Enter) sends through [`HttpClient::send`] and stores the
//! task in `RequestEditor::pending_send`: replacing it cancels the request before it,
//! exactly as AGENTS.md prescribes for a dropped `Task`. `request::Cancel` (Escape)
//! drops the task and aborts the connection. What the send produced lands in the status
//! line — code, reason, time and size — and a failure lands inline under the URL bar as
//! the transport's own [`TransportError::user_message`] and
//! [`TransportError::suggested_fix`], verbatim, never re-worded here (AGENTS.md — errors
//! reach the user). A TLS failure's inline display additionally offers "Disable
//! verification for this request" (REQ-HTTP-10): it pins `verify_tls` off on this
//! request's draft and sends again, with the warning badge beside Send for as long as
//! the override is on. No global setting is ever changed from here.
//!
//! `Decision:` the editor sends through the [`RequestClient`] global instead of holding
//! a client of its own: `new_request`'s app-level handler constructs `RequestEditor` and
//! has no transport to build a client from, and this step's Edits list keeps
//! `new_request.rs` untouched. The app installs the client (S26w builds it on
//! `ReqwestTransport`); tests install one backed by `http_engine::test_support`'s
//! `FakeTransport`. `request_editor::init` cannot install one either: the transport is
//! an app-level wiring decision.

use std::time::Duration;

use gpui::{App, Context, Entity, Global, Role, Window};
use http_engine::{
    HttpClient, PipelineError, Response, ResponseBody, TransportError, resolve_request,
};
use pustman_actions::request;
use ui::prelude::*;
use variables::{DynamicValues, ScopeStack};

use crate::RequestEditor;
use crate::settings_tab::SettingChange;

/// Test hook: the button that sends the request, when nothing is in flight.
pub const SEND_BUTTON_SELECTOR: &str = "send-button";

/// Test hook: the same button while a request is in flight, when it reads "Cancel".
pub const CANCEL_BUTTON_SELECTOR: &str = "cancel-button";

/// Test hook: the status line under the URL bar.
pub const STATUS_LINE_SELECTOR: &str = "status-line";

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

/// What the status line shows for the last response: the code, the reason phrase, the
/// total time and the body size (REQ-RESP-01). A fact the transport did not measure is
/// [`None`] and renders as "—", never estimated.
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

/// The semantic the status code's class is drawn in (the brief's mapping: 2xx success,
/// 3xx info, 4xx warning, 5xx error). The theme's semantic tokens only — the meaning is
/// also spelled out by the code and reason text next to it, never carried by color
/// alone.
fn status_class_color(status: u16) -> Color {
    match status {
        100..=199 | 300..=399 => Color::Info,
        200..=299 => Color::Success,
        400..=499 => Color::Warning,
        _ => Color::Error,
    }
}

/// The total time as the status line spells it: the sub-second range in milliseconds.
fn format_time(total: Option<Duration>) -> String {
    match total {
        None => String::from("—"),
        Some(total) if total < Duration::from_secs(1) => format!("{} ms", total.as_millis()),
        Some(total) => format!("{:.2} s", total.as_secs_f64()),
    }
}

/// The size as the status line spells it. No formatter existed in `ui` or `http_engine`
/// when this landed (both were searched), so this is the one place that formats bytes.
fn format_size(size: Option<u64>) -> String {
    match size {
        None => String::from("—"),
        Some(bytes) if bytes < 1_000 => format!("{bytes} B"),
        Some(bytes) if bytes < 1_000_000 => format!("{:.1} kB", bytes as f64 / 1_000.),
        Some(bytes) => format!("{:.1} MB", bytes as f64 / 1_000_000.),
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

    /// What the status line shows for the last response, if a send has completed.
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
    /// still in flight replaces — and so cancels — the request before it.
    pub fn start_send(&mut self, cx: &mut Context<Self>) {
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
        // Environments are not wired into the editor yet, so the draft resolves against
        // no variables: unresolved names stay literal and are sent as they are
        // (REQ-VAR-07). The warnings a later step surfaces are dropped here on purpose.
        let resolved =
            match resolve_request(&self.draft, &ScopeStack::new(), &mut DynamicValues::new()) {
                Ok((resolved, _warnings)) => resolved,
                Err(error) => {
                    self.fail_send(&error, cx);
                    return;
                }
            };
        let send_task = client.update(cx, |client, cx| client.send(resolved, cx));
        self.inline_error = None;
        self.tls_offer_error = None;
        self.pending_send = Some(cx.spawn(async move |this, cx| {
            let result = send_task.await;
            let landed = this.update(cx, |editor, cx| editor.finish_send(result, cx));
            if landed.is_err() {
                // Handled: the tab closed while the request was in flight; there is no
                // status line left to write to.
            }
        }));
        cx.notify();
    }

    /// Cancel the send in flight: dropping `pending_send` discards its result and
    /// [`HttpClient::cancel`] drops the task that holds the connection (REQ-HTTP-12).
    /// A user cancel is not a failure — no error is shown.
    pub fn cancel_send(&mut self, cx: &mut Context<Self>) {
        self.pending_send = None;
        if let Some(client) = cx
            .try_global::<RequestClient>()
            .map(|client| client.0.clone())
        {
            client.update(cx, |client, cx| client.cancel(cx));
        }
        cx.notify();
    }

    /// A send that never reached the transport failed while being prepared: show why
    /// inline, exactly as a transport failure is shown.
    fn fail_send(&mut self, error: &PipelineError, cx: &mut Context<Self>) {
        self.pending_send = None;
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
    /// errors reach the user). A TLS failure keeps its offer form in `tls_offer_error`,
    /// so "Disable verification for this request" renders for it — and, because the
    /// shown error must be that very failure, never for another error that replaces
    /// `inline_error` outside a send.
    fn show_transport_error(&mut self, error: &TransportError, cx: &mut Context<Self>) {
        let inline = InlineError::from_transport(error);
        self.tls_offer_error = matches!(error, TransportError::Tls { .. }).then(|| inline.clone());
        self.inline_error = Some(inline);
        cx.notify();
    }

    /// The send ended: record the response in the status line, or the failure inline.
    fn finish_send(&mut self, result: Result<Response, TransportError>, cx: &mut Context<Self>) {
        self.pending_send = None;
        match result {
            Ok(response) => {
                self.status_line = Some(StatusLine::from_response(&response));
                self.inline_error = None;
                self.tls_offer_error = None;
            }
            // A cancelled or replaced send is not a failure to report (REQ-HTTP-12).
            Err(TransportError::Cancelled) => {}
            Err(error) => self.show_transport_error(&error, cx),
        }
        cx.notify();
    }

    /// Everything under the URL bar: the inline failure first — where the send happened
    /// — then the status line and the Send/Cancel button.
    pub(super) fn render_send_area(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .gap(DynamicSpacing::Base02.rems(cx))
            .children(
                self.inline_error
                    .clone()
                    .map(|error| self.render_inline_error(&error, cx)),
            )
            .child(
                h_flex()
                    .gap(DynamicSpacing::Base03.rems(cx))
                    .justify_between()
                    .min_w_0()
                    .child(self.render_status(cx))
                    .child(
                        h_flex()
                            .gap(DynamicSpacing::Base03.rems(cx))
                            .items_center()
                            .children(self.render_tls_badge(cx))
                            .child(self.render_send_button(cx)),
                    ),
            )
            .into_any_element()
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

    /// The status line: code (colored by class), reason, total time and size; "Sending…"
    /// while a request is in flight.
    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = match (&self.status_line, self.is_sending()) {
            (_, true) => Label::new(SharedString::new_static("Sending…"))
                .size(LabelSize::Small)
                .color(Color::Muted)
                .into_any_element(),
            (Some(line), false) => h_flex()
                .gap(DynamicSpacing::Base03.rems(cx))
                .min_w_0()
                .child(
                    Label::new(SharedString::from(line.status.to_string()))
                        .size(LabelSize::Small)
                        .color(status_class_color(line.status)),
                )
                .child(
                    Label::new(SharedString::from(line.reason.clone()))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    Label::new(SharedString::from(format_time(line.total)))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    Label::new(SharedString::from(format_size(line.size)))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element(),
            (None, false) => Label::new(SharedString::new_static("No response yet"))
                .size(LabelSize::Small)
                .color(Color::Hint)
                .into_any_element(),
        };
        div()
            .min_w_0()
            .debug_selector(|| STATUS_LINE_SELECTOR.to_string())
            .child(content)
            .into_any_element()
    }

    /// The Send button — "Cancel" while a request is in flight, with the keystroke that
    /// does the same thing spelled out next to the label.
    fn render_send_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let in_flight = self.is_sending();
        let label = self.send_button_label();
        let button = Button::new("send-request", SharedString::new_static(label))
            .style(if in_flight {
                ButtonStyle::Outlined
            } else {
                ButtonStyle::Filled
            })
            .size(ButtonSize::Medium)
            .key_binding(if in_flight {
                KeyBinding::for_action(&request::Cancel)
            } else {
                KeyBinding::for_action(&request::Send)
            })
            .tooltip(Tooltip::text(if in_flight {
                SharedString::new_static("Cancel the request in flight")
            } else {
                SharedString::new_static("Send this request")
            }))
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

#[cfg(test)]
#[path = "send_tests.rs"]
mod tests;
