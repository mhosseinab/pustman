//! Draft autosave (PRD REQ-NFR-05): a draft typed into a tab reaches `storage` within
//! two seconds — and only when it must. [`crate::Restore`]'s observers hear the pane and
//! the docks, but a draft typed into an item notifies nobody (not even the pane), so the
//! save has to be polled: [`Autosave`] holds one repeating GPUI timer task whose tick
//! writes the window's dirty tabs through the restore path ([`Restore::save_now`], which
//! reuses `Restore`'s own `snapshot`/`write_snapshot` — there is no second serializer).
//! A tick that finds nothing dirty writes nothing: the store is not rewritten every 2 s
//! for nothing.
//!
//! The task lives on [`Autosave`], whose handle the window's wiring owns next to its
//! [`Restore`] handle (the same contract [`Restore::attach`] gives its caller): dropping
//! the handle drops the task and stops the timer, and the loop also ends by itself once
//! the window's restore or workspace is gone.

use gpui::{App, AppContext, Context, Entity, Task, WeakEntity};

use crate::Workspace;
use crate::restore::{Restore, SAVE_DEBOUNCE};

#[cfg(test)]
#[path = "autosave_tests.rs"]
mod tests;

/// The repeating timer that keeps dirty tab drafts saved (REQ-NFR-05) — one per window;
/// see the module docs for why a timer at all and where the task lives.
pub struct Autosave {
    _task: Task<()>,
}

impl Autosave {
    /// Start the draft autosave for one window: every [`SAVE_DEBOUNCE`] — REQ-NFR-05's
    /// 2 s cadence, the same constant the debounced save uses — the dirty persistent
    /// tabs of `workspace` are written through `restore`.
    ///
    /// Contract: like [`Restore::attach`], the caller owns the returned handle for as
    /// long as the window lives; dropping it stops the timer.
    pub fn start(
        restore: &Entity<Restore>,
        workspace: &Entity<Workspace>,
        cx: &mut App,
    ) -> Entity<Self> {
        let restore = restore.downgrade();
        let workspace = workspace.downgrade();
        cx.new(|cx| Self {
            _task: cx.spawn(async move |this, cx| {
                loop {
                    // The timer is a GPUI one on the background executor: the foreground
                    // never sleeps and the test clock drives the cadence (AGENTS.md › async).
                    cx.background_executor().timer(SAVE_DEBOUNCE).await;
                    match this.update(cx, |_, cx| tick(&restore, &workspace, cx)) {
                        Ok(true) => {}
                        // The autosave handle was dropped (its entity is gone), or the
                        // window's restore or workspace went with its window: stop ticking.
                        Ok(false) | Err(_) => break,
                    }
                }
            }),
        })
    }
}

/// One tick: write the window's dirty persistent tabs through the restore path, and say
/// whether to keep ticking. Session-only tabs are never stored, so their dirtiness is
/// not ours to save.
fn tick(
    restore: &WeakEntity<Restore>,
    workspace: &WeakEntity<Workspace>,
    cx: &mut Context<Autosave>,
) -> bool {
    let Some(workspace) = workspace.upgrade() else {
        return false;
    };
    let Some(restore) = restore.upgrade() else {
        return false;
    };
    let dirty = workspace
        .read(cx)
        .center_pane()
        .read(cx)
        .tabs
        .iter()
        .any(|tab| tab.handle.persistent_id(cx).is_some() && tab.handle.is_dirty(cx));
    if dirty {
        restore.update(cx, |restore, cx| restore.save_now(cx));
    }
    true
}
