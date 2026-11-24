//! Read-only workspaces (REQ-WSP-04): a workspace whose `pustman.json` declares a newer
//! major `format_version` than this build writes opens read-only. This module is the
//! user-visible half — [`workspace_store`] already refuses the write itself
//! (`SaveError::ReadOnly`): the banner under the title bar, the read-only flag items
//! query before they let an edit through, and what a save press gets instead of a write.
//!
//! `Decision:` the flag is an app-wide [`Global`] queried through [`is_read_only`], the
//! [`crate::workspace_open::OpenStore`] shape, because v1 runs one window. A
//! multi-window build moves it to a per-window flag: two windows must not share one
//! workspace's read-only-ness.
//!
//! `Decision:` `Save`/`SaveAll` are intercepted on the workspace's root element rather
//! than in an app-level handler: `request_editor`'s save is a global handler and the
//! last one in the dispatch chain — no earlier global handler can stop it. An element
//! handler in the bubble phase runs before every global one and stops there; a writable
//! press calls [`App::propagate`] so the save handlers that write files still run.
//!
//! `Decision:` the watch on the window's [`crate::workspace_open::WorkspaceOpen`] is
//! created from an [`App::observe_new`] hook deferred to the end of the workspace's
//! creation effect cycle: `workspace_open` attaches the opener from its own hook for the
//! same entity, and the defer finds it whichever hook registered first.

use gpui::{App, Context, Entity, Global, Subscription, Window};
use pustman_actions::workspace::{Save, SaveAll};
use ui::prelude::*;

use crate::Workspace;
use crate::workspace_open::WorkspaceOpen;

#[cfg(test)]
#[path = "read_only_tests.rs"]
mod tests;

/// What the banner states — the one fact the user needs at a glance: this workspace is
/// read-only and why (REQ-WSP-04).
pub const READ_ONLY_MESSAGE: &str =
    "Read-only workspace — it was made by a newer version of Pustman, so changes cannot be saved";

/// What a refused save press says. It names the cause so the user knows the press was
/// not lost, and what to do about it (AGENTS.md › errors reach the user).
pub const SAVE_REFUSED_MESSAGE: &str = "Nothing was saved — this workspace is read-only because it was made by a newer version \
     of Pustman";

/// The banner's stable element id.
const BANNER_ID: &str = "read-only-banner";

/// The read-only banner under the title bar: [`READ_ONLY_MESSAGE`] in the tone of a
/// caution, built from the design system's [`Banner`] so light and dark stay correct.
pub fn banner() -> Banner {
    Banner::new(BANNER_ID, READ_ONLY_MESSAGE).severity(Severity::Warning)
}

/// The read-only flag: set from the open workspace's [`workspace_store::WorkspaceSnapshot`]
/// (which derives it from `format_version`), queried by items that must not let an edit
/// through in a read-only workspace.
#[derive(Default)]
struct ReadOnlyFlag(bool);

impl Global for ReadOnlyFlag {}

/// The read-only flag items query: `true` while this window's workspace declares a newer
/// major `format_version` (REQ-WSP-04). Items disable their editing on it — text inputs
/// go `set_read_only`, mutating actions stay quiet — and save presses get
/// [`SAVE_REFUSED_MESSAGE`] instead of a write.
pub fn is_read_only(cx: &App) -> bool {
    cx.try_global::<ReadOnlyFlag>().is_some_and(|flag| flag.0)
}

/// Set the flag every consumer of [`is_read_only`] sees. Written unconditionally: a
/// writable workspace that opens after a read-only one must clear it.
fn set_read_only(read_only: bool, cx: &mut App) {
    cx.set_global(ReadOnlyFlag(read_only));
}

/// The one [`App::observe_new`] hook for the whole app and the subscription that keeps
/// it alive, with the double-init guard (the `crate::workspace_open::Openers` shape).
#[derive(Default)]
struct Wiring {
    started: bool,
    _subscriptions: Vec<Subscription>,
}

impl Global for Wiring {}

impl Workspace {
    /// Follow this window's [`WorkspaceOpen`]: sync the flag from its snapshot now and on
    /// every [`crate::workspace_open::WorkspaceOpenEvent`], so the banner and the items'
    /// editing
    /// track the open workspace (REQ-WSP-04).
    pub(crate) fn watch_open(&mut self, opener: &Entity<WorkspaceOpen>, cx: &mut Context<Self>) {
        self.sync_read_only(opener, cx);
        let subscription = cx.subscribe(opener, |workspace, opener, _event, cx| {
            workspace.sync_read_only(&opener, cx);
        });
        self._subscriptions.push(subscription);
    }

    /// The open workspace's read-only flag, as the snapshot the opener holds reports it.
    fn sync_read_only(&mut self, opener: &Entity<WorkspaceOpen>, cx: &mut Context<Self>) {
        let read_only = opener
            .read(cx)
            .snapshot()
            .is_some_and(|snapshot| snapshot.read_only);
        set_read_only(read_only, cx);
        cx.notify();
    }

    /// `workspace::Save` (Ctrl-S / Cmd-S): a read-only workspace does nothing except say
    /// so (the press never reaches a writer); a writable one propagates to the save
    /// handlers that write files (`request_editor`'s app-level handler).
    pub(crate) fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.refuse_save_when_read_only(cx);
    }

    /// `workspace::SaveAll` is refused exactly like [`Self::save`] in a read-only
    /// workspace.
    pub(crate) fn save_all(&mut self, _: &SaveAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.refuse_save_when_read_only(cx);
    }

    /// The read-only save press: one toast and the action stops here. A writable press
    /// propagates onward to whatever saves.
    fn refuse_save_when_read_only(&mut self, cx: &mut Context<Self>) {
        if is_read_only(cx) {
            self.show_toast(SAVE_REFUSED_MESSAGE, Severity::Warning, None, cx);
        } else {
            cx.propagate();
        }
    }
}

/// Wire the read-only watch into the app's init order. Must run after
/// `crate::workspace_open::init` attaches each window's opener; the deferred hook in the
/// listener keeps the two independent of registration order anyway.
pub fn init(cx: &mut App) {
    if cx.default_global::<Wiring>().started {
        return;
    }
    cx.default_global::<Wiring>().started = true;
    let subscription = cx.observe_new::<Workspace>(|_workspace, _window, cx| {
        let workspace = cx.entity();
        // Deferred to the end of this effect cycle: `workspace_open`'s own hook attaches
        // the window's opener while these listeners run, so by the defer it is there.
        cx.defer(move |cx| {
            let Some(opener) = WorkspaceOpen::for_window(workspace.entity_id(), cx) else {
                // No opener means no workspace folder of this window's own: nothing to
                // be read-only about.
                return;
            };
            workspace.update(cx, |workspace, cx| workspace.watch_open(&opener, cx));
        });
    });
    cx.default_global::<Wiring>()
        ._subscriptions
        .push(subscription);
}
