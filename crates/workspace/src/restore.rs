//! Layout and tab restore (PRD REQ-UI-01/02, REQ-NFR-05): what a window looked like when
//! it last closed comes back when it opens. [`Restore`] watches the pane and the two
//! docks and keeps `storage`'s `open_tabs` and `layout` rows up to date — debounced, on a
//! background task, and flushed once more when the window closes — and applies them back
//! at window open through the caller's [`ItemFactory`]. A tab whose item is gone is
//! reported in a toast, not restored.
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

#[cfg(test)]
#[path = "restore_atomic_tests.rs"]
mod atomic_tests;

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
    /// The pending write — the debounced save, the autosave write or the closing flush.
    /// Each new one replaces the task, and dropping the old task cancels the write it had
    /// not started — one write per burst of changes.
    pending_save: Option<Task<()>>,
    /// The snapshot the last successful write stored — what "already saved" means for
    /// [`Self::start_write`]'s comparison. Only a write that landed sets it, so a write
    /// that failed or was cancelled is retried rather than skipped (issue #97).
    last_written: Option<Snapshot>,
    /// Set while a restore is landing, so its own tab pushes and dock changes do not
    /// schedule a save of what was just read.
    is_restoring: bool,
    /// Whether the saved set has been read back (or reported unreadable). Until then the
    /// window's tabs are not the saved ones yet, and a close must not flush them over the
    /// set in `storage` — what is stored is the more complete state (issue #85).
    restore_landed: bool,
    _subscriptions: Vec<Subscription>,
}

impl Restore {
    /// Restore the saved layout and tabs of `workspace_id` into `workspace`'s window now,
    /// and from then on keep them saved whenever the pane or a dock changes.
    ///
    /// Contract: the caller owns the returned handle for as long as the window lives;
    /// dropping it stops the watching and the saving. The items are rebuilt by `factory`
    /// — a tab whose factory returns `None` is reported in one toast and left out.
    /// Attaching also hands the workspace its close hook, so the app's own close button
    /// flushes through the same path a platform close takes (issue #85).
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
        workspace.update(cx, |workspace, _cx| {
            workspace.set_restore(restore.downgrade())
        });
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
        // Closing the window flushes the save the debounce may not have reached yet, and
        // that write then finishes the close (the shape Zed uses for a close handled
        // asynchronously, `crates/zed/src/zed.rs`): the write cannot be lost to the
        // process exiting behind the last closed window.
        window.on_window_should_close(cx, {
            let restore = cx.entity().downgrade();
            move |window, cx| {
                restore
                    .update(cx, |restore, cx| restore.flush_on_close(window, cx))
                    .unwrap_or(true)
            }
        });
        Self {
            store,
            workspace_id,
            workspace: workspace.downgrade(),
            factory,
            pending_save: None,
            last_written: None,
            is_restoring: false,
            restore_landed: false,
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
                    // Nothing was applied and nothing will be: the window's tabs are its
                    // own from here on, and closing may save them.
                    if let Err(_released) = this.update(cx, |restore, cx| {
                        restore.report(READ_FAILURE_MESSAGE, Severity::Error, cx);
                        restore.restore_landed = true;
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
        cx.defer_in(window, |restore, _window, _cx| {
            restore.is_restoring = false;
            restore.restore_landed = true;
        });
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
    /// runs on the background executor through [`Self::start_write`]; a failed one shows
    /// the fixed message and stays user-visible, never only logged.
    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.is_restoring {
            return;
        }
        self.pending_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            // The snapshot is taken when the quiet ends, so changes made while the
            // debounce ran are part of the same write.
            let started = this.update(cx, |restore, cx| restore.start_write(cx));
            let Ok(Some(write)) = started else {
                // The restore entity is gone with its window, or nothing has changed
                // since the last write: there is nothing to save.
                return;
            };
            if let Err(_error) = write.await
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
    /// a tick with nothing changed since the last write writes nothing (issue #97), and
    /// a failed write shows the same fixed message as the debounced path.
    pub(crate) fn save_now(&mut self, cx: &mut Context<Self>) {
        if self.is_restoring {
            return;
        }
        let Some(write) = self.start_write(cx) else {
            return;
        };
        self.pending_save = Some(cx.spawn(async move |this, cx| {
            if let Err(_error) = write.await
                && let Err(_released) = this.update(cx, |restore, cx| {
                    restore.report(WRITE_FAILURE_MESSAGE, Severity::Error, cx);
                })
            {
                // Handled: the workspace went away with its window; no one is left to
                // tell that the write failed.
            }
        }));
    }

    /// The window is closing: write the tabs and layout now rather than after
    /// [`SAVE_DEBOUNCE`] — a change made inside the debounce window would otherwise be
    /// lost with the window — and finish the close once that write landed. Both close
    /// paths land here: a platform close through `on_window_should_close`, and the
    /// title-bar Close control through [`crate::Workspace::close_window`] (issue #85).
    /// Returns whether the close can happen right away; `false` hands the close to the
    /// write, which has to land before the process exits behind the last closed window.
    /// A write that fails keeps the window open with its unsaved drafts and the fixed
    /// message, and the next close tries again — closing must never lose the drafts
    /// silently.
    pub(crate) fn flush_on_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.restore_landed {
            // The saved set has not been read back yet — see `restore_landed`.
            return true;
        }
        let Some(write) = self.start_write(cx) else {
            return true;
        };
        self.pending_save = Some(cx.spawn_in(window, async move |this, cx| {
            if let Err(_error) = write.await {
                if let Err(_released) = this.update_in(cx, |restore, _window, cx| {
                    restore.report(WRITE_FAILURE_MESSAGE, Severity::Error, cx);
                }) {
                    // Handled: the window is gone; no one is left to tell that the
                    // write failed.
                }
                return;
            }
            if let Err(_released) =
                this.update_in(cx, |_restore, window, _cx| window.remove_window())
            {
                // Handled: the window closed itself while the write was landing.
            }
        }));
        false
    }

    /// The write itself, on the background executor: [`write_snapshot`] of the current
    /// layout and tabs with the store handle and workspace id it needs. The debounced
    /// save, the autosave and the closing flush all run this one path; dropping the
    /// returned task with `pending_save` cancels the write it had not finished.
    ///
    /// A snapshot nothing has changed since the last landed write is not written again
    /// (`None`): an unchanged dirty draft would otherwise be rewritten on every autosave
    /// tick (issue #97). What a landed write stored is recorded as [`Self::last_written`],
    /// so every caller compares against the same notion of "already saved".
    fn start_write(&mut self, cx: &mut Context<Self>) -> Option<Task<anyhow::Result<()>>> {
        let snapshot = self.snapshot(cx)?;
        if self.last_written.as_ref() == Some(&snapshot) {
            return None;
        }
        let written = snapshot.clone();
        let store = Arc::clone(&self.store);
        let workspace_id = self.workspace_id.clone();
        Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { write_snapshot(&store, &workspace_id, &snapshot) })
                .await;
            if result.is_ok()
                && let Err(_released) =
                    this.update(cx, |restore, _cx| restore.last_written = Some(written))
            {
                // Handled: the workspace went away with its window right after the write
                // landed; what is stored is what was written.
            }
            result
        }))
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct LayoutDocument {
    left_dock: DockLayout,
    bottom_dock: DockLayout,
    #[serde(default)]
    active_item_id: Option<String>,
}

/// One dock's place in the layout document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct DockLayout {
    is_open: bool,
    size_px: f32,
}

/// One tab on its way to or from `open_tabs`.
#[derive(Clone, PartialEq)]
struct TabSnapshot {
    item_id: String,
    pinned: bool,
    draft: Option<String>,
}

/// One window's saved state. `layout` is `None` when no layout row was written yet (or
/// the row is not one of ours): the docks then keep their defaults and the tabs still
/// restore.
#[derive(Clone, PartialEq)]
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

/// Replace one workspace's saved state with `snapshot`, touching no other workspace. The
/// tab set is replaced by one transaction ([`LocalStore::replace_workspace_tabs`]): the
/// rows that are gone and the rows that arrive land together, so a task interrupted
/// halfway leaves the previous set in place instead of half of each (PRD REQ-UI-02).
fn write_snapshot(
    store: &Mutex<LocalStore>,
    workspace_id: &str,
    snapshot: &Snapshot,
) -> anyhow::Result<()> {
    let mut store_guard = lock_store(store)?;
    let rows = snapshot
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            Ok(OpenTab {
                workspace_id: workspace_id.to_owned(),
                position: i64::try_from(index).map_err(|_error| write_failure())?,
                item_id: tab.item_id.clone(),
                pinned: tab.pinned,
                // The draft is the item's own opaque text (REQ-NFR-05); the column holds
                // it as a JSON string document.
                draft_json: tab.draft.clone().map(serde_json::Value::String),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    store_guard
        .replace_workspace_tabs(workspace_id, &rows)
        .map_err(|_error| write_failure())?;
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
