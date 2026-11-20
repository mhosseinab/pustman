//! `request_editor` — the request tab (PRD REQ-HTTP-01..04): a [`RequestEditor`] item
//! holding the [`HttpRequest`] draft, with the URL bar's method picker and URL field on
//! top, the send/status area (`send`) under it, and the request tab strip (Params now;
//! later steps append) over the tab content (`params`).
//!
//! `Decision:` the step's scope stops at the URL bar: `pending_send`, `inline_error` and
//! the status line are declared here and filled by S26b's send work, so the send task
//! has exactly one home to live in (replacing it cancels the in-flight request).

mod key_value_table;
mod method;
mod new_request;
mod params;
pub mod send;
pub mod url_bar;

use api_model::HttpRequest;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render, SharedString,
    Subscription, Task, Window,
};
use ui::prelude::*;
use workspace::Item;

use params::{Params, ParamsEvent};
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
    /// The request tab strip (Params now; S27b+ append the other tabs).
    tab_bar: Entity<TabBar>,
    /// The Params tab: the query and path-variable tables (S27a).
    params: Entity<Params>,
    /// The send task (S26b): replacing it cancels the request in flight, because
    /// dropping a `Task` cancels it.
    pending_send: Option<Task<()>>,
    /// Why the last send failed, shown under the URL bar.
    inline_error: Option<InlineError>,
    /// What the status line shows for the last response.
    status_line: Option<StatusLine>,
    _url_bar_subscription: Subscription,
    _params_subscription: Subscription,
}

impl RequestEditor {
    /// An editor for `request`, with its method and URL already in the bar. The request
    /// starts clean: it is dirty once the bar applies an edit.
    pub fn new(request: HttpRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url_bar =
            cx.new(|cx| UrlBar::new(request.method.clone(), request.url.clone(), window, cx));
        let url_bar_subscription = cx.subscribe_in(&url_bar, window, Self::on_url_bar_event);
        let params = cx.new(|cx| Params::new(&request, url_bar.clone(), window, cx));
        let params_subscription = cx.subscribe_in(&params, window, Self::on_params_event);
        let tab_bar = cx.new(TabBar::new);
        tab_bar.update(cx, |bar, cx| {
            bar.set_tabs(
                vec![
                    Tab::new(
                        SharedString::new_static("request-tab-params"),
                        SharedString::new_static("Params"),
                    )
                    .toggle_state(true),
                ],
                cx,
            )
        });
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            saved: request.clone(),
            draft: request,
            url_bar,
            tab_bar,
            params,
            pending_send: None,
            inline_error: None,
            status_line: None,
            _url_bar_subscription: url_bar_subscription,
            _params_subscription: params_subscription,
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

    /// The Params tab.
    pub fn params(&self) -> &Entity<Params> {
        &self.params
    }

    /// Move focus to the URL field — `request::FocusUrlBar` (Ctrl-L / Cmd-L).
    pub fn focus_url_bar(&self, window: &mut Window, cx: &mut App) {
        self.url_bar.update(cx, |bar, cx| bar.focus_url(window, cx));
    }

    /// The bar applied an edit, so the draft follows it and the tab may turn dirty.
    /// The URL change is handed to the Params tab, which keeps its query table in step
    /// (S27a) — the one handler for the bar's events, never a second subscription.
    fn on_url_bar_event(
        &mut self,
        _url_bar: &Entity<UrlBar>,
        event: &UrlBarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            UrlBarEvent::UrlChanged => {
                let url = self.url_bar.read(cx).url(cx);
                self.params.update(cx, |params, cx| {
                    params.url_changed(&url, window, cx);
                });
                self.draft.url = url;
            }
            UrlBarEvent::MethodChanged(method) => self.draft.method = method.clone(),
        }
        cx.notify();
    }

    /// A table in the Params tab changed: the draft's key/value rows follow it.
    fn on_params_event(
        &mut self,
        _params: &Entity<Params>,
        event: &ParamsEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ParamsEvent::RowsChanged => {
                self.draft.params = self.params.read(cx).query_rows(cx);
                self.draft.path_variables = self.params.read(cx).path_rows(cx);
            }
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
            // The request tab strip over the tab content. Params is the only tab for
            // now; S27b+ append tabs and switch the content below on the selection.
            .child(self.tab_bar.clone())
            .child(self.params.clone())
    }
}

/// Wire the request editor into the app: the app-level handlers behind
/// `workspace::NewRequest` and `request::FocusUrlBar` (`new_request`).
pub fn init(cx: &mut App) {
    new_request::init(cx);
}
