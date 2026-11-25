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

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use api_model::ItemId;
use gpui::{
    AnyElement, App, Context, Entity, EntityId, FontWeight, Global, Subscription, WeakEntity,
};
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
    locations_of(snapshot).remove(id)
}

/// Every request's trail in `snapshot`, computed in one pass: the containers are
/// collected and ordered once, then each request takes the ones whose directory holds
/// its own.
fn locations_of(snapshot: &WorkspaceSnapshot) -> BTreeMap<ItemId, Vec<String>> {
    let mut containers: Vec<(usize, &Path, String)> = snapshot
        .items
        .values()
        .filter(|item| matches!(item.body, ItemBody::Collection(_) | ItemBody::Folder(_)))
        .filter_map(|item| {
            let own_dir = item.path.as_deref()?.parent()?;
            Some((own_dir.components().count(), own_dir, item.name()))
        })
        .collect();
    containers.sort();
    snapshot
        .items
        .iter()
        .filter(|(_, item)| matches!(item.body, ItemBody::Request(_)))
        .map(|(id, request)| {
            let trail = containers
                .iter()
                .filter(|(_, own_dir, _)| request.dir.starts_with(own_dir))
                .map(|(_, _, name)| name.clone())
                .collect();
            (*id, trail)
        })
        .collect()
}

/// One window's workspace as the breadcrumbs see it: every request's trail, computed
/// once per snapshot and shared by all the window's editors (never once per editor).
struct OpenerLocations {
    locations: BTreeMap<ItemId, Vec<String>>,
    editors: Vec<WeakEntity<RequestEditor>>,
    /// Recomputes `locations` when the window opens a folder or its snapshot changes.
    _subscription: Subscription,
}

/// The init guard, the new-editor observation, and the shared trails per window.
#[derive(Default)]
struct Wiring {
    started: bool,
    by_opener: HashMap<EntityId, OpenerLocations>,
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

fn current_locations(opener: &Entity<WorkspaceOpen>, cx: &App) -> BTreeMap<ItemId, Vec<String>> {
    opener
        .read(cx)
        .snapshot()
        .map(locations_of)
        .unwrap_or_default()
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
    let opener_id = opener.entity_id();
    if !cx.global::<Wiring>().by_opener.contains_key(&opener_id) {
        let locations = current_locations(&opener, cx);
        let subscription = cx.subscribe(&opener, move |opener, event, cx| match event {
            WorkspaceOpenEvent::Opened { .. } | WorkspaceOpenEvent::SnapshotChanged => {
                refresh_opener(opener_id, &opener, cx)
            }
        });
        cx.global_mut::<Wiring>().by_opener.insert(
            opener_id,
            OpenerLocations {
                locations,
                editors: Vec::new(),
                _subscription: subscription,
            },
        );
    }
    let id = editor.read(cx).draft.id;
    let location = cx
        .global_mut::<Wiring>()
        .by_opener
        .get_mut(&opener_id)
        .and_then(|entry| {
            entry.editors.push(editor.downgrade());
            entry.locations.get(&id).cloned()
        });
    editor.update(cx, |editor, cx| editor.set_location(location, cx));
}

/// The window's snapshot changed: recompute its trails once and hand each live editor
/// its own; editors that closed are dropped from the list.
fn refresh_opener(opener_id: EntityId, opener: &Entity<WorkspaceOpen>, cx: &mut App) {
    let locations = current_locations(opener, cx);
    let editors = match cx.global_mut::<Wiring>().by_opener.get_mut(&opener_id) {
        Some(entry) => {
            entry.editors.retain(|editor| editor.upgrade().is_some());
            entry.locations = locations.clone();
            entry.editors.clone()
        }
        None => return,
    };
    for editor in editors.iter().filter_map(WeakEntity::upgrade) {
        editor.update(cx, |editor, cx| {
            let location = locations.get(&editor.draft.id).cloned();
            editor.set_location(location, cx);
        });
    }
}

impl RequestEditor {
    fn set_location(&mut self, location: Option<Vec<String>>, cx: &mut Context<Self>) {
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

#[cfg(test)]
#[path = "breadcrumb_wiring_tests.rs"]
mod wiring_tests;
