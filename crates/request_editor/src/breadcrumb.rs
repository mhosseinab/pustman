//! The breadcrumb row over the URL bar (mockup section 1): where the request lives —
//! `Collection / Folder / Request`, the request's own name in bold — or "Unsaved
//! request" while it has no file in the open workspace, with Save (its shortcut beside
//! it) and Copy as cURL at the right. Both buttons run the editor's existing paths
//! (`save`'s [`RequestEditor::save`], `request::CopyAsCurl`'s handler).
//!
//! `Decision:` the location is read from the folders the request's file sits in —
//! every collection and folder whose own directory contains the request's directory
//! ([`location_of`]) — not from `collections_panel`'s `order` parent map, which this
//! crate cannot depend on (collections_panel depends on request_editor). The workspace
//! format nests an item's file under its container's directory, so the two agree.
//! It is recomputed when the window's workspace opens or its snapshot changes, never
//! in `render`.

use api_model::ItemId;
use gpui::{AnyElement, App, Context, Entity, FontWeight, Global, Subscription};
use pustman_actions::request::CopyAsCurl;
use pustman_actions::workspace::Save;
use ui::prelude::*;
use workspace::{Workspace, WorkspaceOpen, WorkspaceOpenEvent};
use workspace_store::{ItemBody, WorkspaceSnapshot};

use crate::RequestEditor;

/// Test hook: the breadcrumb row.
pub const BREADCRUMB_SELECTOR: &str = "request-breadcrumb";

/// Test hook: the breadcrumb's Save button.
pub const BREADCRUMB_SAVE_SELECTOR: &str = "breadcrumb-save";

/// Test hook: the breadcrumb's Copy as cURL button.
pub const BREADCRUMB_CURL_SELECTOR: &str = "breadcrumb-copy-curl";

/// What the breadcrumb says while the request has no file in the open workspace.
pub const UNSAVED_LABEL: &str = "Unsaved request";

/// The collection and folder names the request `id` sits under, outermost first, or
/// `None` when the snapshot holds no request with that id (not saved there yet).
pub fn location_of(snapshot: &WorkspaceSnapshot, id: &ItemId) -> Option<Vec<String>> {
    let request = snapshot.items.get(id)?;
    if !matches!(request.body, ItemBody::Request(_)) {
        return None;
    }
    let mut containers: Vec<(usize, String)> = snapshot
        .items
        .values()
        .filter(|item| matches!(item.body, ItemBody::Collection(_) | ItemBody::Folder(_)))
        .filter_map(|item| {
            let own_dir = item.path.as_deref()?.parent()?;
            request
                .dir
                .starts_with(own_dir)
                .then(|| (own_dir.components().count(), item.name()))
        })
        .collect();
    containers.sort();
    Some(containers.into_iter().map(|(_, name)| name).collect())
}

/// The init guard and the new-editor observation behind it (`disk_change`'s shape).
#[derive(Default)]
struct Wiring {
    started: bool,
    /// Kept so the observation keeps firing (AGENTS.md).
    _subscriptions: Vec<Subscription>,
}

impl Global for Wiring {}

/// Give every request tab its breadcrumb from its window's open workspace.
pub fn init(cx: &mut App) {
    if cx.default_global::<Wiring>().started {
        return;
    }
    cx.default_global::<Wiring>().started = true;
    let subscription = cx.observe_new::<RequestEditor>(|_editor, _window, cx| {
        let entity = cx.entity();
        // Deferred: the window lookup needs the creation update clear (`disk_change`).
        cx.defer(move |cx| wire_editor(&entity, cx));
    });
    cx.default_global::<Wiring>()
        ._subscriptions
        .push(subscription);
}

fn wire_editor(editor: &Entity<RequestEditor>, cx: &mut App) {
    let Some(opener) = cx
        .with_window(editor.entity_id(), |window, cx| {
            window
                .root::<Workspace>()
                .flatten()
                .and_then(|workspace| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        })
        .flatten()
    else {
        // Handled: the window closed, or a bare window without workspace wiring — the
        // request stays "Unsaved request".
        return;
    };
    editor.update(cx, |editor, cx| editor.follow_location(&opener, cx));
}

impl RequestEditor {
    /// Track where this request lives in `opener`'s workspace from now on.
    pub(crate) fn follow_location(
        &mut self,
        opener: &Entity<WorkspaceOpen>,
        cx: &mut Context<Self>,
    ) {
        self._location_subscription =
            Some(
                cx.subscribe(opener, |editor, opener, event, cx| match event {
                    WorkspaceOpenEvent::Opened { .. } | WorkspaceOpenEvent::SnapshotChanged => {
                        editor.refresh_location(&opener, cx)
                    }
                }),
            );
        self.refresh_location(opener, cx);
    }

    fn refresh_location(&mut self, opener: &Entity<WorkspaceOpen>, cx: &mut Context<Self>) {
        let location = opener
            .read(cx)
            .snapshot()
            .and_then(|snapshot| location_of(snapshot, &self.draft.id));
        if location != self.location {
            self.location = location;
            cx.notify();
        }
    }

    /// The collection and folder names over this request, `None` while unsaved.
    pub fn location(&self) -> Option<&[String]> {
        self.location.as_deref()
    }

    /// The breadcrumb row: the request's place, then Save and Copy as cURL.
    pub(super) fn render_breadcrumb(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut trail = h_flex()
            .flex_1()
            .min_w_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .overflow_hidden();
        match &self.location {
            Some(ancestors) => {
                for name in ancestors {
                    trail = trail
                        .child(
                            Label::new(SharedString::from(name.clone()))
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .truncate(),
                        )
                        .child(
                            Label::new(SharedString::new_static("/"))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        );
                }
                trail = trail.child(
                    div().min_w_0().font_weight(FontWeight::SEMIBOLD).child(
                        Label::new(SharedString::from(self.draft.name.clone()))
                            .size(LabelSize::Small)
                            .truncate(),
                    ),
                );
            }
            None => {
                trail = trail.child(
                    Label::new(SharedString::new_static(UNSAVED_LABEL))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                );
            }
        }
        h_flex()
            .debug_selector(|| BREADCRUMB_SELECTOR.to_string())
            .w_full()
            .min_w_0()
            .items_center()
            .justify_between()
            .gap(DynamicSpacing::Base06.rems(cx))
            .child(trail)
            .child(
                h_flex()
                    .flex_none()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .child(
                        div()
                            .debug_selector(|| BREADCRUMB_SAVE_SELECTOR.to_string())
                            .child(
                                Button::new("breadcrumb-save", SharedString::new_static("Save"))
                                    .style(ButtonStyle::Outlined)
                                    .key_binding(KeyBinding::for_action(&Save))
                                    .tooltip(Tooltip::for_action("Save this request", &Save))
                                    .on_click(cx.listener(|editor, _event, window, cx| {
                                        editor.save(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| BREADCRUMB_CURL_SELECTOR.to_string())
                            .child(
                                Button::new(
                                    "breadcrumb-copy-curl",
                                    SharedString::new_static("Copy as cURL"),
                                )
                                .style(ButtonStyle::Outlined)
                                .tooltip(Tooltip::for_action(
                                    "Copy this request as a cURL command",
                                    &CopyAsCurl,
                                ))
                                .on_click(cx.listener(
                                    |editor, _event, _window, cx| editor.copy_as_curl(cx),
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "breadcrumb_tests.rs"]
mod tests;
