//! The tree's problem rows (REQ-COLLAB-02): the error badge and the message
//! `"<file>:<line> <reason>"` on the tree row, and the details popover the badge opens.
//!
//! The message names the line the problem is at. `workspace_store` carries the line for
//! an invalid-JSON file but not for a merge-conflicted one — it reports the file, not
//! the marker — so the first marker's line is read off the foreground once per snapshot
//! ([`Problems::refresh`]) and folded into the rows when it lands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui::{
    Anchor, AnyElement, ClickEvent, Context, ElementId, FocusHandle, Focusable, ParentElement,
    Pixels, Point, Role, SharedString, StatefulInteractiveElement, Styled, Task, Window, anchored,
    deferred, div,
};
use ui::prelude::*;
use workspace_store::{ItemProblem, ItemProblemKind};

use crate::tree::{CollectionsTree, TreeRow};

/// Test hook: the details popover, as [`gpui::VisualTestContext::debug_bounds`] sees it.
pub(crate) const DETAILS_SELECTOR: &str = "problem-details";

/// The width of the details popover — wide enough for a file path on one line, narrow
/// enough to sit beside the tree it explains.
const DETAILS_WIDTH_PX: f32 = 320.;

/// The problem state one [`CollectionsTree`] carries: the merge-conflict lines read off
/// the foreground, the one open details popover (the [`ui::components::context_menu::ContextMenu`]'s
/// one-at-a-time shape), and the read in flight — dropping the task cancels it when a
/// newer snapshot supersedes it.
pub(crate) struct Problems {
    /// The first merge-conflict marker's 1-based line per conflicted file, relative to
    /// the workspace root.
    marker_lines: BTreeMap<PathBuf, usize>,
    details: Option<Details>,
    /// The details popover's focus: it answers Escape itself and hands focus back to
    /// the tree when it closes.
    focus_handle: FocusHandle,
    marker_task: Option<Task<()>>,
}

/// The one open details popover: what it says, and where it anchors in the window
/// ([`gpui::anchored`]'s own frame).
#[derive(Clone)]
struct Details {
    path: SharedString,
    summary: SharedString,
    detail: SharedString,
    guidance: SharedString,
    position: Point<Pixels>,
}

impl Problems {
    pub(crate) fn new(cx: &mut Context<CollectionsTree>) -> Self {
        Self {
            marker_lines: BTreeMap::new(),
            details: None,
            focus_handle: cx.focus_handle(),
            marker_task: None,
        }
    }

    /// One tree row per problem file: the words its badge (`summary`) and its tooltip
    /// (`detail`) carry, and what its details popover explains (`guidance`).
    pub(crate) fn rows(&self, problems: &[ItemProblem]) -> Vec<TreeRow> {
        problems
            .iter()
            .map(|problem| TreeRow::Problem {
                path: SharedString::from(problem.path.to_string_lossy().into_owned()),
                summary: SharedString::new_static(summary_for(&problem.kind)),
                detail: SharedString::from(self.message(problem)),
                guidance: SharedString::new_static(guidance_for(&problem.kind)),
            })
            .collect()
    }

    /// Read the conflicted files' marker lines off the foreground and fold them into
    /// the rows when they land. A problem the read cannot resolve keeps its message
    /// without a line: the file's problem is already on its row either way.
    pub(crate) fn refresh(
        &mut self,
        root: &Path,
        problems: &[ItemProblem],
        cx: &mut Context<CollectionsTree>,
    ) {
        // A newer snapshot supersedes whatever the last read was finding; dropping the
        // task cancels it (AGENTS.md › dropping a Task cancels it).
        self.marker_task = None;
        self.marker_lines.clear();
        let conflicted: Vec<PathBuf> = problems
            .iter()
            .filter(|problem| matches!(problem.kind, ItemProblemKind::ConflictMarkers))
            .map(|problem| problem.path.clone())
            .collect();
        if conflicted.is_empty() {
            return;
        }
        let root = root.to_path_buf();
        self.marker_task = Some(cx.spawn(async move |this, cx| {
            let marker_lines = cx
                .background_spawn(async move {
                    conflicted
                        .into_iter()
                        .filter_map(|path| {
                            let text = match std::fs::read_to_string(root.join(&path)) {
                                Ok(text) => text,
                                Err(error) => {
                                    // Handled: the row keeps its message without a
                                    // line — the file's problem is already on the row.
                                    log::debug!("problem detail for {path:?}: {error}");
                                    return None;
                                }
                            };
                            first_conflict_marker_line(&text).map(|line| (path, line))
                        })
                        .collect()
                })
                .await;
            if this
                .update(cx, |tree, cx| {
                    tree.problems.marker_lines = marker_lines;
                    tree.reflatten(cx);
                })
                .is_err()
            {
                // Handled: the panel went away with its window before the read landed.
            }
        }));
    }

    /// The tooltip message: `"<file>:<line> <reason>"` — the file, the line the problem
    /// is at, then the problem's own words. A problem no line belongs to (a duplicate id
    /// is a fact about the whole file) names the file and the reason alone; a merge
    /// conflict's line comes from the file itself ([`Problems::refresh`]).
    fn message(&self, problem: &ItemProblem) -> String {
        let path = problem.path.to_string_lossy();
        let line = match &problem.kind {
            ItemProblemKind::InvalidJson { line, .. } => Some(*line),
            ItemProblemKind::ConflictMarkers => self.marker_lines.get(&problem.path).copied(),
            ItemProblemKind::DuplicateId { .. } => None,
        };
        match line {
            Some(line) => format!("{path}:{line} {}", problem.kind),
            None => format!("{path} {}", problem.kind),
        }
    }
}

impl CollectionsTree {
    /// A problem row's error badge: the summary in an error badge, the message in its
    /// tooltip (with the hint that teaches how the details open), and the details
    /// popover on click. The mouse and the keyboard go through one handler — GPUI
    /// synthesizes a click on the focused badge when Enter or Space is pressed.
    pub(crate) fn problem_badge(
        &self,
        path: SharedString,
        summary: SharedString,
        detail: SharedString,
        guidance: SharedString,
        row_key: &ElementId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let badge_selector = format!("{row_key}-badge");
        let open = Details {
            path,
            summary: summary.clone(),
            detail: detail.clone(),
            guidance,
            position: Point::default(),
        };
        h_flex()
            .id(ElementId::from(format!("{row_key}-badge-button")))
            .debug_selector(move || badge_selector)
            .role(Role::Button)
            .aria_label(detail.clone())
            .flex_none()
            .items_center()
            .cursor_pointer()
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(cx.theme().colors().border_focused))
            .tooltip(Tooltip::with_meta(detail, "Open the details").builder())
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                // The row underneath must not treat this as its own click (the
                // shape `ui::components::button` activates in).
                cx.stop_propagation();
                this.toggle_problem_details(open.clone(), event.position(), window, cx);
            }))
            .child(Badge::new(summary).severity(Severity::Error))
            .into_any_element()
    }

    /// Open one problem row's details popover — or close it again when the badge of the
    /// row already showing it is the one clicked.
    fn toggle_problem_details(
        &mut self,
        details: Details,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .problems
            .details
            .as_ref()
            .is_some_and(|open| open.path == details.path)
        {
            self.close_problem_details(cx);
            return;
        }
        let mut details = details;
        details.position = position;
        self.problems.details = Some(details);
        self.problems.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Close the details popover without moving focus — the click that closed it chose
    /// where the keyboard goes next.
    pub(crate) fn close_problem_details(&mut self, cx: &mut Context<Self>) {
        if self.problems.details.take().is_some() {
            cx.notify();
        }
    }

    /// Escape over the details popover: close it and hand focus back to the tree.
    fn cancel_problem_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_problem_details(cx);
        self.focus_handle(cx).focus(window, cx);
    }

    /// The open details popover as a floating overlay at its click position, or nothing
    /// while no popover is open (the shape [`crate::crud`]'s row menu floats in).
    pub(crate) fn details_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let details = self.problems.details.clone()?;
        let position = details.position;
        Some(
            div()
                .absolute()
                .top(px(0.))
                .left_0()
                .child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .position(position)
                            .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                            .child(
                                v_flex()
                                    .id(ElementId::from(format!(
                                        "problem-details-{}",
                                        details.path
                                    )))
                                    .debug_selector(|| String::from(DETAILS_SELECTOR))
                                    .role(Role::Dialog)
                                    .track_focus(&self.problems.focus_handle)
                                    .focus_visible(|style| {
                                        style.border_color(cx.theme().colors().border_focused)
                                    })
                                    .elevation_2(cx)
                                    .occlude()
                                    .w(px_to_rems(DETAILS_WIDTH_PX))
                                    .gap(DynamicSpacing::Base02.rems(cx))
                                    .p(DynamicSpacing::Base04.rems(cx))
                                    .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                                        this.cancel_problem_details(window, cx);
                                    }))
                                    .child(Badge::new(details.summary).severity(Severity::Error))
                                    .child(Label::new(details.detail).size(LabelSize::Small))
                                    .child(
                                        Label::new(details.guidance)
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            ),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }
}

/// The badge's few words: what kind of problem the file has.
fn summary_for(kind: &ItemProblemKind) -> &'static str {
    match kind {
        ItemProblemKind::InvalidJson { .. } => "Invalid JSON",
        ItemProblemKind::ConflictMarkers => "Merge conflict",
        ItemProblemKind::DuplicateId { .. } => "Duplicate id",
    }
}

/// What the details popover explains: what the problem means and what resolves it —
/// the file is never overwritten (REQ-COLLAB-02), so the way out is always the user's.
fn guidance_for(kind: &ItemProblemKind) -> &'static str {
    match kind {
        ItemProblemKind::InvalidJson { .. } => {
            "Pustman holds the file out of the tree until it parses. Fix the JSON where \
             the error is; the file is never overwritten."
        }
        ItemProblemKind::ConflictMarkers => {
            "Merge-conflict markers make the file unreadable as JSON. Resolve the \
             conflict in your Git tool; Pustman never overwrites the file (REQ-COLLAB-02)."
        }
        ItemProblemKind::DuplicateId { .. } => {
            "A rename/rename merge left two files carrying one id. Both halves stay on \
             disk and visible; give one of the files its own id to resolve it \
             (REQ-COLLAB-02)."
        }
    }
}

/// The first merge-conflict marker's 1-based line in `text`.
fn first_conflict_marker_line(text: &str) -> Option<usize> {
    text.lines()
        .position(is_conflict_marker)
        .map(|index| index + 1)
}

/// A merge-conflict marker at the start of a line: `^(<{7}|>{7})( |$)` (REQ-COLLAB-02),
/// the same shape `workspace_store` detects a conflicted file by.
fn is_conflict_marker(line: &str) -> bool {
    ["<<<<<<<", ">>>>>>>"].iter().any(|marker| {
        line.strip_prefix(marker)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    })
}

#[cfg(test)]
#[path = "problems_tests.rs"]
mod tests;
