//! `request_editor` — the request tab (PRD REQ-HTTP-01..04): a [`RequestEditor`] item
//! holding the [`HttpRequest`] draft, with the URL bar's method picker and URL field on
//! top, the send/status area (`send`) under it, and the request tab strip (Params and
//! Headers now; later steps append) over the tab content (`params`, `headers`).
//!
//! `Decision:` the step's scope stops at the URL bar: `pending_send`, `inline_error` and
//! the status line are declared here and filled by S26b's send work, so the send task
//! has exactly one home to live in (replacing it cancels the in-flight request).

mod curl_paste;
mod headers;
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

use headers::{Headers, HeadersEvent};
use params::{Params, ParamsEvent};
use send::{InlineError, StatusLine};
use url_bar::{UrlBar, UrlBarEvent};

/// The key context the editor answers the keyboard in; `assets/keymaps/` binds
/// `request::Send`, `request::Cancel` and `request::FocusUrlBar` against it.
pub const REQUEST_EDITOR_CONTEXT: &str = "RequestEditor";

/// The Params tab's index in the request tab strip.
const PARAMS_TAB_INDEX: usize = 0;

/// The Headers tab's index in the request tab strip.
const HEADERS_TAB_INDEX: usize = 1;

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
    /// The request tab strip (Params and Headers; later tabs append).
    tab_bar: Entity<TabBar>,
    /// Which request tab's content shows: the [`TabBar`]'s selection index.
    active_tab: usize,
    /// The Params tab: the query and path-variable tables (S27a).
    params: Entity<Params>,
    /// The Headers tab: the user's rows and the request's auto headers (S27b).
    headers: Entity<Headers>,
    /// The request as it was before the last cURL import — what "Undo import" restores
    /// in one step (S27c). `None` while no import can be undone.
    pre_import_request: Option<HttpRequest>,
    /// The send task (S26b): replacing it cancels the request in flight, because
    /// dropping a `Task` cancels it.
    pending_send: Option<Task<()>>,
    /// Why the last send failed, shown under the URL bar.
    inline_error: Option<InlineError>,
    /// What the status line shows for the last response.
    status_line: Option<StatusLine>,
    _url_bar_subscription: Subscription,
    _params_subscription: Subscription,
    _headers_subscription: Subscription,
    _tab_bar_subscription: Subscription,
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
        let headers = cx.new(|cx| Headers::new(&request, window, cx));
        let headers_subscription = cx.subscribe_in(&headers, window, Self::on_headers_event);
        let tab_bar = cx.new(TabBar::new);
        tab_bar.update(cx, |bar, cx| {
            bar.set_tabs(
                vec![
                    Tab::new(
                        SharedString::new_static("request-tab-params"),
                        SharedString::new_static("Params"),
                    )
                    .toggle_state(true),
                    Tab::new(
                        SharedString::new_static("request-tab-headers"),
                        SharedString::new_static("Headers"),
                    ),
                ],
                cx,
            )
        });
        let tab_bar_subscription = cx.subscribe_in(&tab_bar, window, Self::on_tab_bar_event);
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            saved: request.clone(),
            draft: request,
            url_bar,
            tab_bar,
            active_tab: PARAMS_TAB_INDEX,
            params,
            headers,
            pre_import_request: None,
            pending_send: None,
            inline_error: None,
            status_line: None,
            _url_bar_subscription: url_bar_subscription,
            _params_subscription: params_subscription,
            _headers_subscription: headers_subscription,
            _tab_bar_subscription: tab_bar_subscription,
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

    /// The Headers tab.
    pub fn headers(&self) -> &Entity<Headers> {
        &self.headers
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
        self.refresh_auto_headers(cx);
        cx.notify();
    }

    /// The request tab strip moved or activated a tab: its selection decides which
    /// tab's content shows. The bar stamps the tabs' active state itself.
    fn on_tab_bar_event(
        &mut self,
        _tab_bar: &Entity<TabBar>,
        event: &TabBarEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = match event {
            TabBarEvent::Selected { index } | TabBarEvent::Activated { index } => *index,
        };
        if self.active_tab != index {
            self.active_tab = index;
            cx.notify();
        }
    }

    /// A table in the Headers tab changed: the draft's header rows follow it, and the
    /// auto headers recompute (a user header of the same name suppresses an auto one).
    fn on_headers_event(
        &mut self,
        _headers: &Entity<Headers>,
        event: &HeadersEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            HeadersEvent::RowsChanged => self.draft.headers = self.headers.read(cx).rows(cx),
        }
        self.refresh_auto_headers(cx);
        cx.notify();
    }

    /// The draft changed: the auto-header list follows it (S27b). Recomputed on every
    /// edit — never in `render`.
    fn refresh_auto_headers(&mut self, cx: &mut Context<Self>) {
        self.headers
            .update(cx, |headers, cx| headers.draft_changed(&self.draft, cx));
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
        self.refresh_auto_headers(cx);
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
            .on_action(cx.listener(Self::handle_copy_as_curl))
            .size_full()
            .min_w_0()
            // The URL bar, wrapped: the cURL paste is captured on this wrapper, which
            // is on the dispatch path exactly while the URL bar has focus (`curl_paste`).
            .child(
                div()
                    .capture_action::<text_input::input::Paste>(
                        cx.listener(Self::handle_paste_curl),
                    )
                    .child(self.url_bar.clone()),
            )
            .children(self.render_import_undo(cx))
            // Under the URL bar, where the send happened: the inline failure and then
            // the status line with the Send/Cancel button.
            .child(self.render_send_area(cx))
            // The request tab strip over the tab content: the selection decides which
            // tab's content shows below.
            .child(self.tab_bar.clone())
            .map(|this| match self.active_tab {
                HEADERS_TAB_INDEX => this.child(self.headers.clone()),
                _ => this.child(self.params.clone()),
            })
    }
}

/// Wire the request editor into the app: the app-level handlers behind
/// `workspace::NewRequest` and `request::FocusUrlBar` (`new_request`).
pub fn init(cx: &mut App) {
    new_request::init(cx);
}
