//! The app-level handlers behind `workspace::NewRequest` and `request::FocusUrlBar`.
//!
//! `Decision:` both are app-level handlers ([`init`] → `App::on_action`), the shape
//! `command_palette` uses for `palette::Toggle`: `Workspace` registers its actions on
//! its own root element and this crate may not edit `crates/workspace` or
//! `crates/pustman`, so there is no element of ours for the keymap to dispatch to
//! before these run. The handlers defer out of the action dispatch (a window cannot be
//! updated while it is on the update stack) and work from the active window's
//! [`Workspace`].

use std::collections::BTreeMap;

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::{App, AppContext};
use pustman_actions::request::FocusUrlBar;
use pustman_actions::workspace::NewRequest;
use workspace::Workspace;

use crate::RequestEditor;

/// Register the two handlers the request editor answers outside its own element tree.
pub fn init(cx: &mut App) {
    cx.on_action(open_request_tab);
    cx.on_action(focus_url_bar);
}

/// `NewRequest` opens a fresh request in a tab and puts the cursor in its URL bar —
/// which is empty, so that is where the typing starts (REQ-UI-02/03).
fn open_request_tab(_: &NewRequest, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        if window_handle
            .update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                let editor = cx.new(|cx| RequestEditor::new(fresh_request(), window, cx));
                workspace.update(cx, |workspace, cx| {
                    workspace.center_pane().update(cx, |pane, cx| {
                        pane.add_tab_item(editor.clone(), cx);
                    });
                });
                editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
            })
            .is_err()
        {
            // The window closed between the keystroke and this callback; there is no
            // workspace left to open a tab in and nothing to tell anyone about.
        }
    });
}

/// `Ctrl-L` / `Cmd-L` focuses the URL bar of the request being edited. The dispatch
/// travels from the focus inside the editor (`assets/keymaps/` binds the keystroke in
/// the `RequestEditor` context), so this only ever reaches the active tab.
fn focus_url_bar(_: &FocusUrlBar, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        if window_handle
            .update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                let editor = workspace.read_with(cx, |workspace, cx| {
                    workspace
                        .center_pane()
                        .read(cx)
                        .active_item()
                        .map(|item| item.to_any_view())
                });
                let Some(editor) = editor.and_then(|view| view.downcast::<RequestEditor>().ok())
                else {
                    return;
                };
                editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
            })
            .is_err()
        {
            // The window closed between the keystroke and this callback; there is
            // nothing left to focus.
        }
    });
}

/// The fresh draft `NewRequest` opens: a GET to an empty URL, under the name a new
/// request tab is born with.
pub(crate) fn fresh_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: String::from("Untitled Request"),
        method: Method::Get,
        url: String::new(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::None,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

#[cfg(test)]
#[path = "new_request_tests.rs"]
mod tests;
