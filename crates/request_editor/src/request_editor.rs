//! `request_editor` — the request tab (PRD REQ-HTTP-01..04): a [`RequestEditor`] item
//! holding the [`HttpRequest`] draft, with the URL bar's method picker and URL field on
//! top, the send area (`send`) under it, and the request tab strip (Params, Headers,
//! Body, Auth and Settings now; later steps append) over the tab content (`params`,
//! `headers`, `body_tab`, `auth_tab`, `settings_tab`) — the request side of
//! [`response_split`]'s resizable split, with the response
//! [`response_viewer::ResponseView`] embedded below it (S29e).
//!
//! `Decision:` the step's scope stops at the URL bar: `pending_send`, `inline_error` and
//! the status line are declared here and filled by S26b's send work, so the send task
//! has exactly one home to live in (replacing it cancels the in-flight request).

mod auth_tab;
mod body_tab;
mod curl_paste;
mod headers;
mod key_value_table;
mod method;
mod new_request;
mod params;
mod response_split;
pub mod send;
mod settings_tab;
pub mod url_bar;

use api_model::{HttpRequest, RequestSettings};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render, SharedString,
    Subscription, Task, Window,
};
use response_viewer::ResponseView;
use ui::prelude::*;
use workspace::Item;

use auth_tab::{AuthTab, AuthTabEvent};
use body_tab::{BodyTab, BodyTabEvent};
use headers::{Headers, HeadersEvent};
use params::{Params, ParamsEvent};
use send::{InlineError, StatusLine};
use settings_tab::{SettingChange, SettingsTab, SettingsTabEvent};
use url_bar::{UrlBar, UrlBarEvent};

/// The key context the editor answers the keyboard in; `assets/keymaps/` binds
/// `request::Send`, `request::Cancel` and `request::FocusUrlBar` against it.
pub const REQUEST_EDITOR_CONTEXT: &str = "RequestEditor";

/// The Params tab's index in the request tab strip.
const PARAMS_TAB_INDEX: usize = 0;

/// The Headers tab's index in the request tab strip.
const HEADERS_TAB_INDEX: usize = 1;

/// The Body tab's index in the request tab strip.
const BODY_TAB_INDEX: usize = 2;

/// The Auth tab's index in the request tab strip.
const AUTH_TAB_INDEX: usize = 3;

/// The Settings tab's index in the request tab strip.
const SETTINGS_TAB_INDEX: usize = 4;

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
    /// The request tab strip (Params, Headers and Body; later tabs append).
    tab_bar: Entity<TabBar>,
    /// Which request tab's content shows: the [`TabBar`]'s selection index.
    active_tab: usize,
    /// The Params tab: the query and path-variable tables (S27a).
    params: Entity<Params>,
    /// The Headers tab: the user's rows and the request's auto headers (S27b).
    headers: Entity<Headers>,
    /// The Body tab: the body-mode editors (S28a).
    body_tab: Entity<BodyTab>,
    /// The Auth tab: the auth-type editors (S28b).
    auth_tab: Entity<AuthTab>,
    /// The Settings tab: this request's settings overrides (S28c).
    settings_tab: Entity<SettingsTab>,
    /// The request as it was before the last cURL import — what "Undo import" restores
    /// in one step (S27c). `None` while no import can be undone.
    pre_import_request: Option<HttpRequest>,
    /// The send task (S26b): replacing it cancels the request in flight, because
    /// dropping a `Task` cancels it.
    pending_send: Option<Task<()>>,
    /// Why the last send failed, shown under the URL bar.
    inline_error: Option<InlineError>,
    /// The TLS failure whose inline display carries the "Disable verification for this
    /// request" offer (REQ-HTTP-10). The offer renders only while this is the error on
    /// display: `curl_paste`'s import error replaces `inline_error` outside a send and
    /// must never show it (`send`'s `render_inline_error`).
    tls_offer_error: Option<InlineError>,
    /// What the status line shows for the last response.
    status_line: Option<StatusLine>,
    /// The response region below the request panes (S29e): the entity a send's result
    /// is handed to (`send`'s `finish_send`).
    response_view: Entity<ResponseView>,
    /// The split handle's own focus target — a tab stop whose Up and Down resize the
    /// split (`response_split`).
    split_focus: FocusHandle,
    /// The share of the split the request pane takes, clamped so neither pane collapses
    /// (`response_split`). `Decision:` not persisted — a per-tab layout preference of
    /// this session, so no workspace-format change comes with it.
    split_fraction: f32,
    _url_bar_subscription: Subscription,
    _params_subscription: Subscription,
    _headers_subscription: Subscription,
    _body_tab_subscription: Subscription,
    _auth_tab_subscription: Subscription,
    _settings_tab_subscription: Subscription,
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
        let body_tab = cx.new(|cx| BodyTab::new(&request, window, cx));
        let body_tab_subscription = cx.subscribe_in(&body_tab, window, Self::on_body_tab_event);
        let auth_tab = cx.new(|cx| AuthTab::new(&request, window, cx));
        let auth_tab_subscription = cx.subscribe_in(&auth_tab, window, Self::on_auth_tab_event);
        let settings_tab = cx.new(|cx| SettingsTab::new(&request, window, cx));
        let settings_tab_subscription =
            cx.subscribe_in(&settings_tab, window, Self::on_settings_tab_event);
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
                    Tab::new(
                        SharedString::new_static("request-tab-body"),
                        SharedString::new_static("Body"),
                    ),
                    Tab::new(
                        SharedString::new_static("request-tab-auth"),
                        SharedString::new_static("Auth"),
                    ),
                    Tab::new(
                        SharedString::new_static("request-tab-settings"),
                        SharedString::new_static("Settings"),
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
            body_tab,
            auth_tab,
            settings_tab,
            pre_import_request: None,
            pending_send: None,
            inline_error: None,
            tls_offer_error: None,
            status_line: None,
            response_view: cx.new(ResponseView::new),
            split_focus: cx.focus_handle().tab_stop(true),
            split_fraction: response_split::DEFAULT_SPLIT_FRACTION,
            _url_bar_subscription: url_bar_subscription,
            _params_subscription: params_subscription,
            _headers_subscription: headers_subscription,
            _body_tab_subscription: body_tab_subscription,
            _auth_tab_subscription: auth_tab_subscription,
            _settings_tab_subscription: settings_tab_subscription,
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

    /// The Body tab.
    pub fn body_tab(&self) -> &Entity<BodyTab> {
        &self.body_tab
    }

    /// The Auth tab.
    pub fn auth_tab(&self) -> &Entity<AuthTab> {
        &self.auth_tab
    }

    /// The Settings tab.
    pub fn settings_tab(&self) -> &Entity<SettingsTab> {
        &self.settings_tab
    }

    /// The response view embedded below the request panes (S29e) — where a send's
    /// result is shown.
    pub fn response_view(&self) -> &Entity<ResponseView> {
        &self.response_view
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
        if index == SETTINGS_TAB_INDEX {
            // The Settings tab mirrors the draft's settings and nothing else writes to
            // it: a cURL import or undo replaces the whole request without re-seeding
            // this tab (`curl_paste` is outside its step's Edits), so it re-reads the
            // draft whenever the user comes back to it.
            self.settings_tab
                .update(cx, |tab, cx| tab.reseed(&self.draft, cx));
        }
        if self.active_tab != index {
            self.active_tab = index;
            cx.notify();
        }
    }

    /// A table in the Headers tab changed: the draft's header rows follow it, the
    /// tab's disabled-auto-header list reaches the draft's settings (REQ-HTTP-03), and
    /// the auto headers recompute (a user header of the same name suppresses an auto
    /// one).
    fn on_headers_event(
        &mut self,
        _headers: &Entity<Headers>,
        event: &HeadersEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            HeadersEvent::RowsChanged => {
                self.draft.headers = self.headers.read(cx).rows(cx);
                self.copy_disabled_auto_headers_to_draft(cx);
            }
        }
        self.refresh_auto_headers(cx);
        cx.notify();
    }

    /// The Headers tab's disabled-auto-header list is the new truth for the draft's
    /// (REQ-HTTP-03) — the one place the copy happens. Only that list comes from the
    /// tab; every other [`RequestSettings`] field keeps whatever the draft carries. A
    /// settings that overrides nothing is stored as `None` (which is what `None`
    /// means: inherit the global settings), so a switch flipped off and on again
    /// leaves the request — and the workspace format — exactly as it was.
    fn copy_disabled_auto_headers_to_draft(&mut self, cx: &App) {
        let disabled = self
            .headers
            .read(cx)
            .draft_settings()
            .disabled_auto_headers
            .clone();
        let mut settings = self.draft.settings.take().unwrap_or_default();
        settings.disabled_auto_headers = disabled;
        self.draft.settings = if settings == RequestSettings::default() {
            None
        } else {
            Some(settings)
        };
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

    /// A control in the Settings tab changed one override: write that change into the
    /// draft's settings (REQ-HTTP-09) — the one place they are written from this tab.
    /// `disabled_auto_headers` and `extra` are fields the tab does not own and come
    /// through untouched, and a settings that overrides nothing is stored as `None`
    /// (the `copy_disabled_auto_headers_to_draft` rule), so a setting flipped away
    /// from and back to its default leaves the request exactly as it was.
    fn apply_setting_change(&mut self, change: SettingChange, cx: &mut Context<Self>) {
        let mut settings = self.draft.settings.take().unwrap_or_default();
        change.apply(&mut settings);
        self.draft.settings = (settings != RequestSettings::default()).then_some(settings);
        cx.notify();
    }

    /// The Settings tab changed: its one changed override reaches the draft (the
    /// event carries it, so an edit of one field never touches another).
    fn on_settings_tab_event(
        &mut self,
        _settings_tab: &Entity<SettingsTab>,
        event: &SettingsTabEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SettingsTabEvent::SettingChanged(change) => self.apply_setting_change(*change, cx),
        }
    }

    /// The Body tab changed: the draft's body follows it (REQ-HTTP-04) — the one
    /// place the body is written — and the auto headers recompute, because the body's
    /// mode decides its Content-Type. A cURL import rebuilds this tab from the
    /// imported request (`curl_paste`'s `replace_request`), so the next body edit
    /// extends the imported body instead of overwriting it with stale state.
    fn on_body_tab_event(
        &mut self,
        _body_tab: &Entity<BodyTab>,
        event: &BodyTabEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BodyTabEvent::BodyChanged => self.draft.body = self.body_tab.read(cx).body(cx),
        }
        self.refresh_auto_headers(cx);
        cx.notify();
    }

    /// The Auth tab changed: the draft's auth follows it (REQ-HTTP-06) — the one
    /// place the auth is written — and the auto headers recompute, because the
    /// applied auth adds headers the send pipeline generates.
    fn on_auth_tab_event(
        &mut self,
        _auth_tab: &Entity<AuthTab>,
        event: &AuthTabEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            AuthTabEvent::AuthChanged => self.draft.auth = self.auth_tab.read(cx).auth(),
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
        // The request side of the split: the URL bar and the tabs under it.
        let request_content = v_flex()
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
            // the Send/Cancel button.
            .child(self.render_send_area(cx))
            // The request tab strip over the tab content: the selection decides which
            // tab's content shows below.
            .child(self.tab_bar.clone())
            .map(|this| match self.active_tab {
                HEADERS_TAB_INDEX => this.child(self.headers.clone()),
                BODY_TAB_INDEX => this.child(self.body_tab.clone()),
                AUTH_TAB_INDEX => this.child(self.auth_tab.clone()),
                SETTINGS_TAB_INDEX => this.child(self.settings_tab.clone()),
                _ => this.child(self.params.clone()),
            })
            .into_any_element();
        v_flex()
            .id(ElementId::from(&self.focus_handle))
            .key_context(REQUEST_EDITOR_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::handle_send))
            .on_action(cx.listener(Self::handle_cancel))
            .on_action(cx.listener(Self::handle_copy_as_curl))
            .size_full()
            .min_w_0()
            // The resizable split: the request content above, the embedded response
            // view below (`response_split`).
            .child(self.render_response_split(request_content, cx))
    }
}

/// Wire the request editor into the app: the app-level handlers behind
/// `workspace::NewRequest` and `request::FocusUrlBar` (`new_request`).
pub fn init(cx: &mut App) {
    new_request::init(cx);
}

#[cfg(test)]
#[path = "request_editor_auto_header_tests.rs"]
mod auto_header_tests;

#[cfg(test)]
#[path = "request_editor_curl_reseed_tests.rs"]
mod curl_reseed_tests;
