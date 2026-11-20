//! `request_editor` — the request tab (PRD REQ-HTTP-01..04): a [`RequestEditor`] item
//! holding the [`HttpRequest`] draft, with the URL bar's method picker and URL field on
//! top, and the send/status area (`send`) under it.
//!
//! `Decision:` the step's scope stops at the URL bar: `pending_send`, `inline_error` and
//! the status line are declared here and filled by S26b's send work, so the send task
//! has exactly one home to live in (replacing it cancels the in-flight request).

mod method;
mod new_request;
pub mod send;
pub mod url_bar;

use api_model::HttpRequest;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render, SharedString,
    Subscription, Task, Window,
};
use ui::prelude::*;
use workspace::Item;

use send::{InlineError, StatusLine};
use url_bar::{UrlBar, UrlBarEvent};

/// The key context the editor answers the keyboard in; `assets/keymaps/` binds
/// `request::Send`, `request::Cancel` and `request::FocusUrlBar` against it.
pub const REQUEST_EDITOR_CONTEXT: &str = "RequestEditor";

/// One open request: the draft the tab is editing, the URL bar that edits it, and the
/// fields the status line area below it is drawn from (S26b).
pub struct RequestEditor {
    focus_handle: FocusHandle,
    /// The request being edited — the tab's model and the source the send is built from.
    draft: HttpRequest,
    /// The request as of the last save. The dirty dot compares against it, so an edit
    /// and then its undo leave the tab clean.
    saved: HttpRequest,
    url_bar: Entity<UrlBar>,
    /// The send task (S26b): replacing it cancels the request in flight, because
    /// dropping a `Task` cancels it.
    pending_send: Option<Task<()>>,
    /// Why the last send failed, shown under the URL bar.
    inline_error: Option<InlineError>,
    /// What the status line shows for the last response.
    status_line: Option<StatusLine>,
    _url_bar_subscription: Subscription,
}

impl RequestEditor {
    /// An editor for `request`, with its method and URL already in the bar. The request
    /// starts clean: it is dirty once the bar applies an edit.
    pub fn new(request: HttpRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url_bar =
            cx.new(|cx| UrlBar::new(request.method.clone(), request.url.clone(), window, cx));
        let url_bar_subscription = cx.subscribe_in(&url_bar, window, Self::on_url_bar_event);
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            saved: request.clone(),
            draft: request,
            url_bar,
            pending_send: None,
            inline_error: None,
            status_line: None,
            _url_bar_subscription: url_bar_subscription,
        }
    }

    /// The request this tab is editing.
    pub fn draft(&self) -> &HttpRequest {
        &self.draft
    }

    /// The URL bar this tab edits its method and URL through.
    pub fn url_bar(&self) -> &Entity<UrlBar> {
        &self.url_bar
    }

    /// Move focus to the URL field — `request::FocusUrlBar` (Ctrl-L / Cmd-L).
    pub fn focus_url_bar(&self, window: &mut Window, cx: &mut App) {
        self.url_bar.update(cx, |bar, cx| bar.focus_url(window, cx));
    }

    /// The bar applied an edit, so the draft follows it and the tab may turn dirty.
    fn on_url_bar_event(
        &mut self,
        _url_bar: &Entity<UrlBar>,
        event: &UrlBarEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            UrlBarEvent::UrlChanged => self.draft.url = self.url_bar.read(cx).url(cx),
            UrlBarEvent::MethodChanged(method) => self.draft.method = method.clone(),
        }
        cx.notify();
    }
}

impl Focusable for RequestEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for RequestEditor {
    fn tab_title(&self, _cx: &App) -> SharedString {
        self.draft.name.clone().into()
    }

    fn is_dirty(&self, _cx: &App) -> bool {
        self.draft != self.saved
    }
}

impl Render for RequestEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(ElementId::from(&self.focus_handle))
            .key_context(REQUEST_EDITOR_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::handle_send))
            .on_action(cx.listener(Self::handle_cancel))
            .size_full()
            .min_w_0()
            .child(self.url_bar.clone())
            // Under the URL bar, where the send happened: the inline failure and then
            // the status line with the Send/Cancel button.
            .child(self.render_send_area(cx))
    }
}

/// Wire the request editor into the app: the app-level handlers behind
/// `workspace::NewRequest` and `request::FocusUrlBar` (`new_request`).
pub fn init(cx: &mut App) {
    new_request::init(cx);
}
