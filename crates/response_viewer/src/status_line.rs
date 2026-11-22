//! The response header above the response body (REQ-RESP-01): the "Response" title,
//! the status as a pill and the total time and body size as labelled meta beside it —
//! the Response header the blueprint shows (`06_visuals/workbench_blueprint.svg`, S05).
//!
//! The pill draws in the HTTP class's own hue from the theme's status tokens — one hue
//! per class, never a literal — and the code always spells the class out, so meaning is
//! never colour-only. The texts are computed in [`StatusLine::new`] when the response
//! arrives, never in `render`, and draw with tabular numbers so the row does not jitter
//! between responses.

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, FontFeatures, Hsla};
use http_engine::ResponseBody;
use ui::prelude::*;

use crate::child_id;

/// What the time and size metrics show when the transport could not measure them: an em
/// dash, never a guess (ADR-0003).
const UNMEASURED: &str = "—";

/// The heading over the response facts — the blueprint's Response header
/// (`06_visuals/workbench_blueprint.svg`, S05). The colour is named explicitly: a
/// [`Headline`] with no colour of its own inherits its container's text colour, and this
/// row sets none (`response_viewer.rs` #266).
const RESPONSE_HEADER: &str = "Response";
const RESPONSE_HEADER_COLOR: Color = Color::Default;

/// The debug key the heading renders under — the test hook that lets
/// `VisualTestContext::debug_bounds` prove it is on screen (the `error_panel.rs`
/// pattern). It is derived from the rendered text, so the key and the label cannot
/// drift apart.
fn response_header_selector(title: &str) -> String {
    format!("response-header-{title}")
}

/// The debug key the status pill renders under (see [`response_header_selector`]),
/// derived from the pill's rendered label — code and reason, as the user reads them.
fn status_pill_selector(label: &str) -> String {
    format!("status-pill-{label}")
}

/// The debug key one labelled metric renders under (see [`response_header_selector`]),
/// derived from the rendered label and value — "Time"/"184 ms", "Size"/"244 B".
pub(crate) fn status_meta_selector(label: &str, value: &str) -> String {
    format!("status-meta-{label}-{value}")
}

/// The status line: one response's facts, precomputed into display text.
#[derive(Clone, Debug, IntoElement)]
pub struct StatusLine {
    id: ElementId,
    status: u16,
    reason: SharedString,
    time_text: SharedString,
    size_text: SharedString,
}

impl StatusLine {
    /// The status line for one response, identified like every interactive component —
    /// by the caller's stable id, so two viewers in one frame never share element ids.
    /// `total` is the duration of the whole exchange and `body_size` the body's byte
    /// count, both as the transport measured them.
    pub fn new(
        id: impl Into<ElementId>,
        status: u16,
        reason: impl Into<SharedString>,
        total: Option<Duration>,
        body_size: Option<u64>,
    ) -> Self {
        Self {
            id: id.into(),
            status,
            reason: reason.into(),
            time_text: format_duration(total).into(),
            size_text: format_size(body_size).into(),
        }
    }

    /// The body's measured byte count, fed back when a streamed body's read learns it
    /// after the response arrived (#122): [`body_size`] cannot know a
    /// [`ResponseBody::File`]'s length without touching the disk, so the Size metric
    /// starts unmeasured and the load that measures it updates the precomputed text
    /// here.
    pub fn set_body_size(&mut self, body_size: u64) {
        self.size_text = format_size(Some(body_size)).into();
    }
}

impl RenderOnce for StatusLine {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let pill_label = format!("{} {}", self.status, self.reason);
        h_flex()
            .w_full()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base03.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                div()
                    .debug_selector(|| response_header_selector(RESPONSE_HEADER))
                    .child(
                        Headline::new(RESPONSE_HEADER)
                            .size(HeadlineSize::Small)
                            .color(RESPONSE_HEADER_COLOR),
                    ),
            )
            .child(status_pill(
                child_id(&self.id, "status"),
                pill_label,
                self.status,
                cx,
            ))
            .child(metric(
                child_id(&self.id, "time"),
                "Time",
                self.time_text,
                "Total time",
                cx,
            ))
            .child(metric(
                child_id(&self.id, "size"),
                "Size",
                self.size_text,
                "Body size",
                cx,
            ))
            .child(div().flex_1())
    }
}

/// The color a status code draws in: one theme token per HTTP class. A code outside the
/// five classes — never sent by a conforming server — draws in plain text.
fn status_class_color(status: u16, cx: &App) -> Hsla {
    let status_colors = cx.theme().status();
    match status {
        100..=199 => status_colors.http_informational,
        200..=299 => status_colors.http_success,
        300..=399 => status_colors.http_redirect,
        400..=499 => status_colors.http_client_error,
        500..=599 => status_colors.http_server_error,
        _ => cx.theme().colors().text,
    }
}

/// The body's byte count, when it is known without touching the disk. A streamed body
/// ([`ResponseBody::File`]) is measured by the load that reads it, so its size stays
/// unknown until then — never guessed.
pub(crate) fn body_size(body: &ResponseBody) -> Option<u64> {
    match body {
        ResponseBody::Bytes(bytes) => u64::try_from(bytes.len()).ok(),
        ResponseBody::Spilled { total_len, .. } => Some(*total_len),
        ResponseBody::File { .. } => None,
    }
}

/// The severity family the pill's surface draws from. The HTTP classes are not
/// severities — the hue stays [`status_class_color`]'s — but 2xx–5xx each mean the
/// outcome their severity names (S05: "HTTP response versus transport failure"), and a
/// 1xx carries no outcome yet, so its pill is neutral.
fn status_severity(status: u16) -> Option<Severity> {
    match status {
        200..=299 => Some(Severity::Success),
        300..=399 => Some(Severity::Info),
        400..=499 => Some(Severity::Warning),
        500..=599 => Some(Severity::Error),
        _ => None,
    }
}

/// The status as the blueprint's pill (`06_visuals/workbench_blueprint.svg`, "200 OK"):
/// code and reason in the class hue on the outcome family's banner surface. It is
/// [`ui::Badge`]'s shape at the pack's 12 px metadata size — the badge's 10 px label is
/// its "counts and keybinding hints" size — and it truncates with the full label in its
/// tooltip, because the reason phrase arrives from the wire (S05).
fn status_pill(
    id: ElementId,
    label: impl Into<SharedString>,
    status: u16,
    cx: &App,
) -> impl IntoElement {
    let label: SharedString = label.into();
    let selector = status_pill_selector(&label);
    let tooltip_label = label.clone();
    let code_color = status_class_color(status, cx);
    div()
        .id(id)
        .min_w_0()
        .tooltip(Tooltip::text(tooltip_label).builder())
        .debug_selector(move || selector)
        .child(
            h_flex()
                .min_w_0()
                .items_center()
                .px(DynamicSpacing::Base02.rems(cx))
                .py(DynamicSpacing::Base01.rems(cx))
                .rounded_full()
                .border_1()
                .text_color(code_color)
                .map(|this| match status_severity(status) {
                    Some(severity) => this
                        .bg(severity.background(cx))
                        .border_color(severity.border(cx)),
                    None => this
                        .bg(cx.theme().colors().element_background)
                        .border_color(cx.theme().colors().border_variant),
                })
                .child(Label::new(label).size(LabelSize::Small).truncate()),
        )
}

/// One labelled metric — "Time 184 ms" or "Size 244 B" — with the full fact in its
/// tooltip. The label stays visible: a bare number pinned to the far right edge never
/// says what it measured (S05, #223).
fn metric(
    id: ElementId,
    label: &'static str,
    value: SharedString,
    what: &'static str,
    cx: &App,
) -> impl IntoElement {
    let selector = status_meta_selector(label, &value);
    h_flex()
        .id(id)
        .items_center()
        .gap(DynamicSpacing::Base01.rems(cx))
        .font_ui(cx)
        .text_ui_sm(cx)
        .font_features(tabular_numbers())
        .tooltip(Tooltip::text(what).builder())
        .debug_selector(move || selector)
        .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
        .child(
            Label::new(value)
                .size(LabelSize::Small)
                .color(Color::Default),
        )
}

/// Tabular figures for the time and size metrics, so their digits line up and the row
/// does not jitter between responses.
fn tabular_numbers() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]))
}

/// A duration as the response shows it: milliseconds under a second, seconds above one,
/// minutes and seconds for anything over a minute. The status line's times, the Timeline
/// tab's rows, the Console rows and the History rows show one shape — the shared `ui`
/// pair — with this pane's "unmeasured" text standing in where nothing was measured
/// (S31a review 1, #134).
pub(crate) fn format_duration(total: Option<Duration>) -> String {
    total.map_or_else(|| UNMEASURED.to_owned(), ui::format_duration)
}

/// A byte count as the response shows it: 1024-based `B`, `KB`, `MB`, `GB` —
/// [`format_duration`]'s size twin, from the same shared pair (#134).
pub(crate) fn format_size(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| UNMEASURED.to_owned(), ui::format_size)
}

#[cfg(test)]
#[path = "status_line_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "response_header_tests.rs"]
mod response_header_tests;
