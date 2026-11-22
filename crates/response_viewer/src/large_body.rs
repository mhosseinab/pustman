//! The large-body fallback (REQ-RESP-06): a body over `request.response_pretty_limit_mb`
//! opens Raw with a banner instead of being pretty-printed.
//!
//! Pretty-printing a body far past the limit is heavy work for a view nobody asked for
//! it from, so the pane makes the choice the setting describes: Raw shows, the banner
//! above the lines names the limit — and names no view mode, because the user can still
//! pick Pretty or open Hex/Preview with the banner up (#182) — and, when the body spilled
//! or streamed to disk, one button reveals that file in the system file manager.
//!
//! A body's file path can carry a resolved secret (PRD §7): it is never rendered as
//! text, never part of an element id and never logged.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use http_engine::ResponseBody;
use settings::{Setting, Settings, SettingsStore};
use ui::prelude::*;

use crate::child_id;

/// One megabyte, the unit `request.response_pretty_limit_mb` counts in.
const MEGABYTE: u64 = 1024 * 1024;

/// What the banner shows while a body is over the pretty limit: the limit that was
/// exceeded, and where the body's bytes live on disk when they do.
#[derive(Clone)]
pub(crate) struct LargeBodyNotice {
    /// The configured limit the body went over, in MB — the banner names it.
    limit_mb: u64,
    /// What the banner's button reveals: [`ResponseBody::Spilled`]'s temp file or
    /// [`ResponseBody::File`]'s path. An in-memory body has none, so the banner shows
    /// the notice alone — never a button that leads nowhere.
    file_path: Option<PathBuf>,
}

impl LargeBodyNotice {
    /// The notice for a body over `limit_mb`, with `file_path` behind it (or not).
    pub(crate) fn new(limit_mb: u64, file_path: Option<PathBuf>) -> Self {
        Self {
            limit_mb,
            file_path,
        }
    }

    /// The banner's line: the limit, in the units the setting counts in. Mode-neutral on
    /// purpose (#182): the banner stays on screen when the user picks Pretty or opens
    /// Hex/Preview — the reveal button lives on it — so the wording may name no mode
    /// without going stale against the mode bar.
    pub(crate) fn message(&self) -> SharedString {
        format!("Response is larger than {} MB.", self.limit_mb).into()
    }

    /// Where the banner's button reveals the body's bytes, when there is a file.
    pub(crate) fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }
}

/// Whether a body of `size` bytes is over the pretty limit of `limit_mb` MB
/// (REQ-RESP-06). Pure on purpose: the one place the comparison lives, so the view and
/// its tests cannot disagree about the boundary.
pub(crate) fn exceeds_pretty_limit(size: u64, limit_mb: u64) -> bool {
    size > limit_mb.saturating_mul(MEGABYTE)
}

/// This crate's view of the settings it reads — the `settings` crate's [`Setting`]
/// pattern, like `console::ConsoleSetting`. [`crate::BodyView::new`] registers it before
/// the first read.
#[derive(Clone, Copy)]
pub(crate) struct ResponseViewerSetting {
    /// `request.response_pretty_limit_mb`: megabytes of body that are pretty-printed.
    pub(crate) response_pretty_limit_mb: u64,
}

impl Setting for ResponseViewerSetting {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            response_pretty_limit_mb: settings.request.response_pretty_limit_mb,
        }
    }
}

/// The pretty limit in MB: `request.response_pretty_limit_mb` from the settings store,
/// or the settings crate's own default while the store is not installed (tests, early
/// boot) — the default is documented in one place and never copied here.
pub(crate) fn pretty_limit_mb(cx: &App) -> u64 {
    if cx.has_global::<SettingsStore>() {
        ResponseViewerSetting::get_global(cx).response_pretty_limit_mb
    } else {
        Settings::default().request.response_pretty_limit_mb
    }
}

/// Where `body`'s bytes live on disk, when they do — what the banner's button reveals
/// (REQ-RESP-06). The value is for that button alone (see the module docs).
pub(crate) fn file_path(body: &ResponseBody) -> Option<PathBuf> {
    match body {
        ResponseBody::Bytes(_) => None,
        ResponseBody::File { path, .. } => Some(path.clone()),
        ResponseBody::Spilled { file, .. } => Some(file.path().to_owned()),
    }
}

/// The banner above a body over the pretty limit (REQ-RESP-06): the fact, and — when
/// the body has a file behind it — the button that reveals it. Composed from the design
/// system primitives like [`crate::error_panel::ErrorPanel`]: `crates/ui` has no
/// Callout component to reuse yet.
#[derive(Clone, IntoElement)]
pub(crate) struct LargeBodyBanner {
    id: ElementId,
    message: SharedString,
    file_path: Option<PathBuf>,
    on_reveal: Option<ClickHandler>,
}

impl LargeBodyBanner {
    /// The banner for `notice`, identified like every interactive component by the
    /// caller's stable id (`crate::child_id`).
    pub(crate) fn new(id: impl Into<ElementId>, notice: &LargeBodyNotice) -> Self {
        Self {
            id: id.into(),
            message: notice.message(),
            file_path: notice.file_path.clone(),
            on_reveal: None,
        }
    }

    /// What the reveal button does. It renders only together with a file (see
    /// [`LargeBodyNotice::file_path`]): a banner never offers an action that fails.
    pub(crate) fn on_reveal(
        mut self,
        on_reveal: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_reveal = Some(Rc::new(on_reveal));
        self
    }
}

impl RenderOnce for LargeBodyBanner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let severity = Severity::Info;
        v_flex()
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base02.rems(cx))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(DynamicSpacing::Base03.rems(cx))
                    .px(DynamicSpacing::Base04.rems(cx))
                    .py(DynamicSpacing::Base03.rems(cx))
                    .rounded_md()
                    .border_1()
                    .bg(severity.background(cx))
                    .border_color(severity.border(cx))
                    .text_color(severity.color(cx))
                    .child(Icon::new(IconName::Info).size(IconSize::Small))
                    // The message wraps, never truncates: it is why the pane fell back
                    // and must be readable in full.
                    .child(div().min_w_0().flex_1().child(Label::new(self.message)))
                    .map(|this| match (self.file_path, self.on_reveal) {
                        (Some(_), Some(on_reveal)) => this.child(
                            Button::new(child_id(&self.id, "reveal"), "Open temp file location")
                                .size(ButtonSize::Medium)
                                .start_icon(Icon::new(IconName::FolderOpen))
                                .tooltip(Tooltip::text(
                                    "Show the response body file in your file manager",
                                ))
                                .on_click(move |event, window, cx| on_reveal(event, window, cx)),
                        ),
                        _ => this,
                    }),
            )
    }
}

#[cfg(test)]
#[path = "large_body_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "large_body_banner_tests.rs"]
mod banner_tests;
