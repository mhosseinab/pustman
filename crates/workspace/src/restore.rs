//! Layout and tab restore (PRD REQ-UI-01/02, REQ-NFR-05): what a window looked like when
//! it last closed comes back when it opens. [`Restore`] watches the pane and the two
//! docks and keeps `storage`'s `open_tabs` and `layout` rows up to date — debounced, on a
//! background task — and applies them back at window open through the caller's
//! [`ItemFactory`]. A tab whose item is gone is reported in a toast, not restored.
//!
//! `workspace` knows nothing about the feature crates whose items fill the tabs, so the
//! caller supplies the factory (one closure, not a plugin framework) and owns the handle
//! [`Restore::attach`] returns — dropping it stops the watching and the saving. The store
//! handle and workspace id come in explicitly for the same reason: wiring this into
//! `pustman` is a later step.
//!
//! `storage` is synchronous by design (ADR-0002), so every SQLite call runs on the
//! background executor and the foreground never blocks on it (AGENTS.md › async). Error
//! text from `storage` never reaches the user or a log: it can quote the row it failed
//! on, and a tab draft is unsaved user content (PRD §7) — one fixed message per failure
//! is all that is shown, through the workspace's toast stack.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use gpui::{App, Context, Entity, SharedString, Subscription, Task, WeakEntity, Window};
use serde::{Deserialize, Serialize};
use storage::{Layout, LocalStore, OpenTab, StorageError};
use ui::prelude::*;

use crate::Workspace;
use crate::item::ItemHandle;

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;

/// How long a change to the pane or a dock waits for the next one before the save task
/// writes it — REQ-NFR-05's 2 s autosave cadence.
pub const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// The one message a failed restore read shows; see the module docs for why the
/// `StorageError` behind it is dropped unread.
const READ_FAILURE_MESSAGE: &str = "The saved tabs and layout could not be read";
/// The one message a failed save shows; see [`READ_FAILURE_MESSAGE`].
const WRITE_FAILURE_MESSAGE: &str = "The tabs and layout could not be saved";

/// How [`Restore`] recreates a tab's item: given the stored item id and the draft the
/// item serialized, the caller's feature crate builds the item — or returns `None`, which
/// means the backing item is gone and the tab is not restored (PRD REQ-UI-02). The
/// handle is `Send + Sync` so the factory can live wherever the window's state does.
pub type ItemFactory = Arc<
    dyn Fn(&str, Option<String>, &mut Window, &mut App) -> Option<Box<dyn ItemHandle>>
        + Send
        + Sync,
>;

/// The watcher that keeps one window's layout and open tabs in `storage`, and restores
/// them when the window opens.
pub struct Restore {
    store: Arc<Mutex<LocalStore>>,
    workspace_id: String,
    workspace: WeakEntity<Workspace>,
    factory: ItemFactory,
    /// The debounced save. Each change replaces the task, and dropping the old task
    /// cancels the write it had not started — one write per burst of changes.
    pending_save: Option<Task<()>>,
    /// Set while a restore is landing, so its own tab pushes and dock changes do not
    /// schedule a save of what was just read.
    is_restoring: bool,
    _subscriptions: Vec<Subscription>,
}

impl Restore {
    /// Restore the saved layout and tabs of `workspace_id` into `workspace`'s window now,
    /// and from then on keep them saved whenever the pane or a dock changes.
    ///
    /// Contract: the caller owns the returned handle for as long as the window lives;
    /// dropping it stops the watching and the saving. The items are rebuilt by `factory`
    /// — a tab whose factory returns `None` is reported in one toast and left out.
    pub fn attach(
        workspace: &Entity<Workspace>,
        store: Arc<Mutex<LocalStore>>,
        workspace_id: String,
        factory: ItemFactory,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let restore = cx.new(|cx| Self::new(workspace, store, workspace_id, factory, window, cx));
        restore.update(cx, |restore, cx| restore.restore_now(window, cx));
        restore
    }

    fn new(
        workspace: &Entity<Workspace>,
        store: Arc<Mutex<LocalStore>>,
        workspace_id: String,
        factory: ItemFactory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (pane, left_dock, bottom_dock) = {
            let workspace = workspace.read(cx);
            (
                workspace.center_pane().clone(),
                workspace.left_dock().clone(),
                workspace.bottom_dock().clone(),
            )
        };
        // Every change to the tabs or a dock ends in `cx.notify()` on that entity, so
        // observing them is the whole "on change" hook. `observe_in` — not `observe` —
        // also ties this entity to the window, which is what lets the restore task below
        // reach the window again after its background read.
        let _subscriptions = vec![
            cx.observe_in(&pane, window, |restore, _pane, _window, cx| {
                restore.schedule_save(cx)
            }),
            cx.observe_in(&left_dock, window, |restore, _dock, _window, cx| {
                restore.schedule_save(cx)
            }),
            cx.observe_in(&bottom_dock, window, |restore, _dock, _window, cx| {
                restore.schedule_save(cx)
            }),
        ];
        Self {
            store,
            workspace_id,
            workspace: workspace.downgrade(),
            factory,
            pending_save: None,
            is_restoring: false,
            _subscriptions,
        }
    }

    /// Restore the saved layout and tabs into this window's workspace: read them off the
    /// foreground thread, then rebuild the tabs through the [`ItemFactory`] (REQ-UI-02).
    fn restore_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = Arc::clone(&self.store);
        let workspace_id = self.workspace_id.clone();
        cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read_snapshot(&store, &workspace_id) })
                .await;
            match read {
                Ok(snapshot) => {
                    if let Err(_released) = this.update_in(cx, |restore, window, cx| {
                        restore.apply(snapshot, window, cx)
                    }) {
                        // Handled: the workspace went away with its window before the
                        // restore landed; there is nothing left to restore into.
                    }
                }
                Err(_error) => {
                    // The failure is folded into the fixed message (PRD §7); this is
                    // where it reaches the user (AGENTS.md › errors must reach the user).
                    if let Err(_released) = this.update(cx, |restore, cx| {
                        restore.report(READ_FAILURE_MESSAGE, Severity::Error, cx);
                    }) {
                        // Handled: the workspace went away with its window.
                    }
                }
            }
        })
        .detach();
    }

    /// Land `snapshot` in the window: the docks first, then the tabs in their saved order
    /// with their pins and drafts. A tab the factory cannot rebuild is reported in one
    /// toast and left out (REQ-UI-02).
    fn apply(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        self.is_restoring = true;
        let Some(workspace) = self.workspace.upgrade() else {
            self.is_restoring = false;
            return;
        };
        let Snapshot { layout, tabs } = snapshot;
        if let Some(layout) = &layout {
            workspace.update(cx, |workspace, cx| {
                workspace.left_dock().update(cx, |dock, cx| {
                    dock.set_open(layout.left_dock.is_open, cx);
                    dock.set_size_px(layout.left_dock.size_px, cx);
                });
                workspace.bottom_dock().update(cx, |dock, cx| {
                    dock.set_open(layout.bottom_dock.is_open, cx);
                    dock.set_size_px(layout.bottom_dock.size_px, cx);
                });
            });
        }
        let mut missing = Vec::new();
        let mut first_restored = None;
        let mut restored_active = None;
        for tab in tabs {
            let was_active = layout
                .as_ref()
                .and_then(|layout| layout.active_item_id.as_deref())
                == Some(tab.item_id.as_str());
            let Some(handle) = (self.factory)(&tab.item_id, tab.draft, window, cx) else {
                missing.push(tab.item_id);
                continue;
            };
            let entity_id = handle.entity_id();
            workspace.update(cx, |workspace, cx| {
                workspace.center_pane().update(cx, |pane, cx| {
                    pane.push_tab(handle, None, cx);
                    if tab.pinned {
                        pane.set_pinned(entity_id, true, cx);
                    }
                });
            });
            first_restored.get_or_insert(entity_id);
            if was_active {
                restored_active = Some(entity_id);
            }
        }
        // The saved active tab wins; without one (or when its item is gone) the first tab
        // that came back opens — and when nothing came back, whatever the pane holds is
        // left exactly as it is.
        if let Some(entity_id) = restored_active.or(first_restored) {
            workspace.update(cx, |workspace, cx| {
                workspace
                    .center_pane()
                    .update(cx, |pane, cx| pane.activate_item(entity_id, cx));
            });
        }
        if !missing.is_empty() {
            self.report(
                format!(
                    "Couldn't restore tabs; missing items: {}",
                    missing.join(", ")
                ),
                Severity::Warning,
                cx,
            );
        }
        // Saving is suppressed until the notifications above flush — and effects flush
        // in the order they were queued, so `defer_in` runs after the pane and dock
        // observers this restore just woke. Without that, every restore would schedule a
        // save of the state it just read back.
        cx.defer_in(window, |restore, _window, _cx| restore.is_restoring = false);
    }

    /// The workspace's current layout and tabs as they should be stored. Items without a
    /// [`ItemHandle::persistent_id`] are session-only and stay out (PRD REQ-UI-02), and
    /// the active tab lives in the layout document: `open_tabs` has no column for it.
    fn snapshot(&self, cx: &App) -> Option<Snapshot> {
        let workspace = self.workspace.upgrade()?;
        let workspace = workspace.read(cx);
        let pane = workspace.center_pane().read(cx);
        let tabs = pane
            .tabs
            .iter()
            .filter_map(|tab| {
                let item_id = tab.handle.persistent_id(cx)?.to_string();
                Some(TabSnapshot {
                    item_id,
                    pinned: tab.pinned,
                    draft: tab.handle.serialize_draft(cx),
                })
            })
            .collect();
        let left_dock = workspace.left_dock().read(cx);
        let bottom_dock = workspace.bottom_dock().read(cx);
        Some(Snapshot {
            layout: Some(LayoutDocument {
                left_dock: DockLayout {
                    is_open: left_dock.is_open(),
                    size_px: left_dock.size_px(),
                },
                bottom_dock: DockLayout {
                    is_open: bottom_dock.is_open(),
                    size_px: bottom_dock.size_px(),
                },
                active_item_id: pane
                    .active_item()
                    .and_then(|item| item.persistent_id(cx))
                    .map(|item_id| item_id.to_string()),
            }),
            tabs,
        })
    }

    /// Save the layout and tabs after [`SAVE_DEBOUNCE`] of quiet (REQ-NFR-05). The write
    /// runs on the background executor; a failed one shows the fixed message and stays
    /// user-visible, never only logged.
    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.is_restoring {
            return;
        }
        self.pending_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let saved = this.update(cx, |restore, cx| {
                restore.snapshot(cx).map(|snapshot| {
                    (
                        Arc::clone(&restore.store),
                        restore.workspace_id.clone(),
                        snapshot,
                    )
                })
            });
            let Ok(Some((store, workspace_id, snapshot))) = saved else {
                // The restore entity is gone with its window; there is nothing to save.
                return;
            };
            let write = cx
                .background_executor()
                .spawn(async move { write_snapshot(&store, &workspace_id, &snapshot) })
                .await;
            if let Err(_error) = write
                && let Err(_released) = this.update(cx, |restore, cx| {
                    restore.report(WRITE_FAILURE_MESSAGE, Severity::Error, cx);
                })
            {
                // Handled: the workspace went away with its window; no one is left to
                // tell that the write failed.
            }
        }));
    }

    /// Write the current layout and tabs now — the entry point the draft autosave timer
    /// drives ([`crate::Autosave`], PRD REQ-NFR-05). It reuses [`Self::snapshot`] and
    /// [`write_snapshot`], so there is no second serializer and no second `open_tabs`
    /// writer. While a restore is landing it does nothing (see [`Self::schedule_save`]),
    /// and a failed write shows the same fixed message as the debounced path.
    pub(crate) fn save_now(&mut self, cx: &mut Context<Self>) {
        if self.is_restoring {
            return;
        }
        let Some(snapshot) = self.snapshot(cx) else {
            return;
        };
        let store = Arc::clone(&self.store);
        let workspace_id = self.workspace_id.clone();
        self.pending_save = Some(cx.spawn(async move |this, cx| {
            let write = cx
                .background_executor()
                .spawn(async move { write_snapshot(&store, &workspace_id, &snapshot) })
                .await;
            if let Err(_error) = write
                && let Err(_released) = this.update(cx, |restore, cx| {
                    restore.report(WRITE_FAILURE_MESSAGE, Severity::Error, cx);
                })
            {
                // Handled: the workspace went away with its window; no one is left to
                // tell that the write failed.
            }
        }));
    }

    /// Show `message` through the workspace's toast stack — the one door
    /// [`Workspace::show_toast`] provides (AGENTS.md › errors must reach the user).
    fn report(&self, message: impl Into<SharedString>, level: Severity, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let message = message.into();
        workspace.update(cx, |workspace, cx| {
            workspace.show_toast(message, level, None, cx);
        });
    }
}

/// What the `layout` row holds: the docks' sizes and visibility as small typed fields —
/// never a map, so the document has one stable shape — plus the active tab's persistent
/// id (`open_tabs` has no column for it).
#[derive(Debug, Serialize, Deserialize)]
struct LayoutDocument {
    left_dock: DockLayout,
    bottom_dock: DockLayout,
    #[serde(default)]
    active_item_id: Option<String>,
}

/// One dock's place in the layout document.
#[derive(Debug, Serialize, Deserialize)]
struct DockLayout {
    is_open: bool,
    size_px: f32,
}

/// One tab on its way to or from `open_tabs`.
struct TabSnapshot {
    item_id: String,
    pinned: bool,
    draft: Option<String>,
}

/// One window's saved state. `layout` is `None` when no layout row was written yet (or
/// the row is not one of ours): the docks then keep their defaults and the tabs still
/// restore.
struct Snapshot {
    layout: Option<LayoutDocument>,
    tabs: Vec<TabSnapshot>,
}

/// Read one workspace's saved state. The read runs entirely inside the background task
/// (ADR-0002). A layout row that parses as JSON but not as one of our documents is
/// ignored — local, untrusted data — and the tabs still come back.
fn read_snapshot(store: &Mutex<LocalStore>, workspace_id: &str) -> anyhow::Result<Snapshot> {
    let store_guard = lock_store(store)?;
    let rows = store_guard
        .open_tabs()
        .list(workspace_id)
        .map_err(read_failure)?;
    let layout_row = store_guard
        .layout()
        .list()
        .map_err(read_failure)?
        .into_iter()
        .find(|layout| layout.workspace_id == workspace_id);
    let layout = layout_row.and_then(|row| serde_json::from_value(row.json).ok());
    let tabs = rows
        .into_iter()
        .map(|row| TabSnapshot {
            item_id: row.item_id,
            pinned: row.pinned,
            draft: row.draft_json.map(|value| match value {
                serde_json::Value::String(text) => text,
                other => other.to_string(),
            }),
        })
        .collect();
    Ok(Snapshot { layout, tabs })
}

/// Replace one workspace's saved state with `snapshot`, touching no other workspace.
/// `open_tabs` has no clear-a-workspace call, so the set is replaced by difference: rows
/// whose tab is gone are deleted one by one and the rest are inserted (its `insert`
/// upserts on `(workspace_id, item_id)`). No transaction API exists either, so a task
/// interrupted halfway can leave the rows partly replaced until the next save — the same
/// limitation `http_engine`'s cookie persistence accepts.
fn write_snapshot(
    store: &Mutex<LocalStore>,
    workspace_id: &str,
    snapshot: &Snapshot,
) -> anyhow::Result<()> {
    let store_guard = lock_store(store)?;
    let open_tabs = store_guard.open_tabs();
    let kept: HashSet<&str> = snapshot
        .tabs
        .iter()
        .map(|tab| tab.item_id.as_str())
        .collect();
    for row in open_tabs
        .list(workspace_id)
        .map_err(|_error| write_failure())?
    {
        if !kept.contains(row.item_id.as_str()) {
            open_tabs
                .delete(workspace_id, &row.item_id)
                .map_err(|_error| write_failure())?;
        }
    }
    for (index, tab) in snapshot.tabs.iter().enumerate() {
        let position = i64::try_from(index).map_err(|_error| write_failure())?;
        open_tabs
            .insert(&OpenTab {
                workspace_id: workspace_id.to_owned(),
                position,
                item_id: tab.item_id.clone(),
                pinned: tab.pinned,
                // The draft is the item's own opaque text (REQ-NFR-05); the column holds
                // it as a JSON string document.
                draft_json: tab.draft.clone().map(serde_json::Value::String),
            })
            .map_err(|_error| write_failure())?;
    }
    if let Some(layout) = &snapshot.layout {
        let json = serde_json::to_value(layout).map_err(|_error| write_failure())?;
        store_guard
            .layout()
            .insert(&Layout {
                workspace_id: workspace_id.to_owned(),
                json,
            })
            .map_err(|_error| write_failure())?;
    }
    Ok(())
}

/// The store handle: a poisoned lock is an error, never a panic. The lock is only ever
/// held inside the background task, across calls that cannot yield.
fn lock_store(store: &Mutex<LocalStore>) -> anyhow::Result<MutexGuard<'_, LocalStore>> {
    store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store was left mid-operation by a failed task"))
}

/// The one message a failed tabs or layout read shows, whatever failed in `storage`; see
/// the module docs for why the error behind it is dropped unread.
fn read_failure(_error: StorageError) -> anyhow::Error {
    anyhow::anyhow!(READ_FAILURE_MESSAGE)
}

/// The one message a failed tabs or layout write shows, whatever failed; see
/// [`read_failure`] for why the error behind it is dropped unread.
fn write_failure() -> anyhow::Error {
    anyhow::anyhow!(WRITE_FAILURE_MESSAGE)
}
