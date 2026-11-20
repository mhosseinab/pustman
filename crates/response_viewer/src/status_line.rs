//! The status line above the response body: status code, reason, total time and size
//! (REQ-RESP-01).
//!
//! The status code is colored by its HTTP class from the theme's status tokens — one hue
//! per class, never a literal — and the reason, time and size sit beside it. The texts
//! are computed in [`StatusLine::new`] when the response arrives, never in `render`, and
//! draw with tabular numbers so the row does not jitter between responses.

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, FontFeatures, Hsla};
use http_engine::ResponseBody;
use ui::prelude::*;

use crate::child_id;

/// What the time and size metrics show when the transport could not measure them: an em
/// dash, never a guess (ADR-0003).
const UNMEASURED: &str = "—";

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
}

impl RenderOnce for StatusLine {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let code_color = status_class_color(self.status, cx);
        h_flex()
            .w_full()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base03.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                div()
                    .font_ui(cx)
                    .text_ui(cx)
                    .text_color(code_color)
                    .child(self.status.to_string()),
            )
            .child(
                div()
                    .id(child_id(&self.id, "reason"))
                    .min_w_0()
                    .tooltip(Tooltip::text(self.reason.clone()).builder())
                    .child(Label::new(self.reason).color(Color::Muted).truncate()),
            )
            .child(div().flex_1())
            .child(metric(
                child_id(&self.id, "time"),
                self.time_text,
                "Total time",
                cx,
            ))
            .child(metric(
                child_id(&self.id, "size"),
                self.size_text,
                "Body size",
                cx,
            ))
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

/// One status metric — the time or the size — with the full fact in its tooltip.
fn metric(id: ElementId, value: SharedString, what: &'static str, cx: &App) -> impl IntoElement {
    div()
        .id(id)
        .font_ui(cx)
        .text_ui_sm(cx)
        .font_features(tabular_numbers())
        .text_color(cx.theme().colors().text_muted)
        .tooltip(Tooltip::text(what).builder())
        .child(value)
}

/// Tabular figures for the time and size metrics, so their digits line up and the row
/// does not jitter between responses.
fn tabular_numbers() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]))
}

/// A duration as the status line shows it: milliseconds under a second, seconds above
/// one, minutes and seconds for anything over a minute.
fn format_duration(total: Option<Duration>) -> String {
    let Some(total) = total else {
        return UNMEASURED.to_owned();
    };
    if total < Duration::from_secs(1) {
        format!("{:.0} ms", total.as_secs_f64() * 1000.0)
    } else if total < Duration::from_secs(60) {
        format!("{:.2} s", total.as_secs_f64())
    } else {
        format!("{} m {:02} s", total.as_secs() / 60, total.as_secs() % 60)
    }
}

/// A byte count as the status line shows it: 1024-based `B`, `KB`, `MB`, `GB`.
fn format_size(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return UNMEASURED.to_owned();
    };
    const UNIT: f64 = 1024.0;
    // Display only: the loss of precision past 2^53 bytes cannot show in a size string.
    let bytes = bytes as f64;
    if bytes < UNIT {
        format!("{bytes:.0} B")
    } else if bytes < UNIT * UNIT {
        format!("{:.2} KB", bytes / UNIT)
    } else if bytes < UNIT * UNIT * UNIT {
        format!("{:.2} MB", bytes / UNIT / UNIT)
    } else {
        format!("{:.2} GB", bytes / UNIT / UNIT / UNIT)
    }
}

#[cfg(test)]
#[path = "status_line_tests.rs"]
mod tests;
