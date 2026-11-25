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
pub mod breadcrumb;
mod curl_paste;
pub mod disk_change;
mod headers;
mod key_value_table;
mod method;
mod new_request;
mod params;
mod read_only_wiring;
pub mod request_tabs;
mod response_split;
pub mod save;
pub mod send;
pub mod send_menu;
mod settings_tab;
mod unresolved_warning;
pub mod url_bar;
pub mod variable_autocomplete;
mod variable_highlight;
mod variable_hover;
pub mod variable_line;

use std::sync::Arc;

use api_model::{HttpRequest, ItemId, RequestSettings};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render, SharedString,
    Subscription, Task, Window,
};
use response_viewer::ResponseView;
use ui::prelude::*;
use workspace::{Item, ItemFactory};

use auth_tab::{AuthTab, AuthTabEvent};
use body_tab::{BodyTab, BodyTabEvent};
use headers::{Headers, HeadersEvent};
use params::{Params, ParamsEvent};
use send::{InlineError, StatusLine};
use settings_tab::{SettingChange, SettingsTab, SettingsTabEvent};
use unresolved_warning::UnresolvedWarning;
use url_bar::{UrlBar, UrlBarEvent};
// Re-exported for the app's scope wire (S43w): it hands every editor its keychain
// sources and keys local values by the winning variable's scope.
pub use variable_hover::{SecretSources, scope_kind_of};

/// The key context the editor answers the keyboard in; `assets/keymaps/` binds
/// `request::Send`, `request::Cancel` and `request::FocusUrlBar` against it.
pub const REQUEST_EDITOR_CONTEXT: &str = "RequestEditor";

/// The test selector on the active tab's content, under the request tab strip.
pub(crate) const REQUEST_TAB_CONTENT_SELECTOR: &str = "request-tab-content";

/// The Params tab's index in the request tab strip.
const PARAMS_TAB_INDEX: usize = 0;

/// The Auth tab's index in the request tab strip (mockup order: Params, Auth, …).
const AUTH_TAB_INDEX: usize = 1;

/// The Headers tab's index in the request tab strip.
const HEADERS_TAB_INDEX: usize = 2;

/// The Body tab's index in the request tab strip.
const BODY_TAB_INDEX: usize = 3;

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
    /// The "Changed on disk" bar's state while the file behind this tab changed under a
    /// dirty draft (`disk_change`, REQ-COLLAB-01).
    disk_change: Option<disk_change::DiskChange>,
    /// The disk version "Keep mine" dismissed (`disk_change`): the tab keeps its draft
    /// against it, and the bar stays down while the file still holds exactly it.
    dismissed_disk: Option<HttpRequest>,
    /// The banner's message while this tab's file is gone from disk or cannot be read
    /// (`disk_change`, #308, REQ-COLLAB-02).
    file_gone: Option<SharedString>,
    /// The watcher over this tab's workspace folder and the pump that turns its file
    /// changes into this tab's reloads and bars (`disk_change`). Dropping the tab drops
    /// the watcher and cancels the pump.
    disk_watch: Option<disk_change::DiskWatch>,
    /// Kept so this tab's watcher follows its window's workspace opens (`disk_change`).
    _disk_open_subscription: Option<Subscription>,
    url_bar: Entity<UrlBar>,
    /// The request tab strip (Params, Headers and Body; later tabs append).
    tab_bar: Entity<TabBar>,
    /// Which request tab's content shows: the [`TabBar`]'s selection index.
    active_tab: usize,
    /// The collection and folder names over this request, `None` while it has no file
    /// in the open workspace (`breadcrumb`).
    location: Option<Vec<String>>,
    /// Each `{{variable}}` the request uses and what it resolves to (`variable_line`).
    variable_line: Vec<variable_line::VariableLineEntry>,
    /// The active environment's name, for the variable line (`variable_line`).
    environment_name: Option<String>,
    /// The counts and dots the tab strip shows (`request_tabs`).
    tab_badges: request_tabs::TabBadges,
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
    /// The split Send's chevron menu while it is open (`send_menu`).
    send_menu: Option<send_menu::SendMenu>,
    /// Why the last send failed, shown under the URL bar.
    inline_error: Option<InlineError>,
    /// The TLS failure whose inline display carries the "Disable verification for this
    /// request" offer (REQ-HTTP-10). The offer renders only while this is the error on
    /// display: `curl_paste`'s import error replaces `inline_error` outside a send and
    /// must never show it (`send`'s `render_inline_error`).
    tls_offer_error: Option<InlineError>,
    /// The pre-send unresolved-variable warning bar's state (REQ-VAR-07): [`Some`] while
    /// the bar lists names a send would use without a value (`unresolved_warning`).
    unresolved_warning: Option<UnresolvedWarning>,
    /// Why the last send went out without the credentials its auth was configured for —
    /// incomplete or unsupported (REQ-HTTP-06) — shown under the URL row (`send`).
    auth_warning: Option<String>,
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
    /// The scopes every field highlights its `{{variables}}` against (REQ-VAR-03);
    /// `variable_highlight` owns the API around it (`set_scopes`), shared with the
    /// field modules as an `Arc` because `ScopeStack` is not `Clone`.
    scopes: std::sync::Arc<variables::ScopeStack>,
    /// Where the send's secret values come from (`variable_hover`'s [`SecretSources`]):
    /// the store `http_engine::SendInput` reads and the one door to a secret's keychain
    /// key. The default reads nothing, which is the M1 behaviour; `set_secret_sources`
    /// fills it — and the hover's own copy — when the secret stores are wired (S43w).
    secret_sources: SecretSources,
    /// For a tab opened from History, the id its request was recorded under
    /// ([`RequestEditor::from_history`]); `None` for every other tab.
    history_origin: Option<ItemId>,
    _url_bar_subscription: Subscription,
    _params_subscription: Subscription,
    _headers_subscription: Subscription,
    _body_tab_subscription: Subscription,
    _auth_tab_subscription: Subscription,
    _settings_tab_subscription: Subscription,
    _tab_bar_subscription: Subscription,
    /// Cancels this tab's in-flight send when the tab is released (closed): a send left
    /// running would still write its history row, but its Console entry — written by this
    /// tab when the response lands — could never appear.
    _release_subscription: Subscription,
}

/// The request-derived children of a [`RequestEditor`] — the URL bar and the Params,
/// Headers, Body and Auth tabs — with the subscriptions that wire their events back to
/// the editor. [`RequestEditor::new`] and `curl_paste`'s `replace_request` build them
/// through one shared path (issue #159), so a rebuild cannot drift from the original
/// construction. The Settings tab is not among them: it mirrors the draft and re-seeds
/// itself whenever its tab is activated (`on_tab_bar_event`).
struct RequestFields {
    url_bar: Entity<UrlBar>,
    params: Entity<Params>,
    headers: Entity<Headers>,
    body_tab: Entity<BodyTab>,
    auth_tab: Entity<AuthTab>,
    _url_bar_subscription: Subscription,
    _params_subscription: Subscription,
    _headers_subscription: Subscription,
    _body_tab_subscription: Subscription,
    _auth_tab_subscription: Subscription,
}

impl RequestFields {
    /// Put these fields in `editor`, dropping whatever was there — the old children
    /// and their subscriptions go together.
    fn install(self, editor: &mut RequestEditor) {
        editor.url_bar = self.url_bar;
        editor.params = self.params;
        editor.headers = self.headers;
        editor.body_tab = self.body_tab;
        editor.auth_tab = self.auth_tab;
        editor._url_bar_subscription = self._url_bar_subscription;
        editor._params_subscription = self._params_subscription;
        editor._headers_subscription = self._headers_subscription;
        editor._body_tab_subscription = self._body_tab_subscription;
        editor._auth_tab_subscription = self._auth_tab_subscription;
    }
}

impl RequestEditor {
    /// Build every request-derived field from `request`, each wired to its handler.
    fn build_request_fields(
        request: &HttpRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> RequestFields {
        let url_bar =
            cx.new(|cx| UrlBar::new(request.method.clone(), request.url.clone(), window, cx));
        let _url_bar_subscription = cx.subscribe_in(&url_bar, window, Self::on_url_bar_event);
        let params = cx.new(|cx| Params::new(request, url_bar.clone(), window, cx));
        let _params_subscription = cx.subscribe_in(&params, window, Self::on_params_event);
        let headers = cx.new(|cx| Headers::new(request, window, cx));
        let _headers_subscription = cx.subscribe_in(&headers, window, Self::on_headers_event);
        let body_tab = cx.new(|cx| BodyTab::new(request, window, cx));
        let _body_tab_subscription = cx.subscribe_in(&body_tab, window, Self::on_body_tab_event);
        let auth_tab = cx.new(|cx| AuthTab::new(request, window, cx));
        let _auth_tab_subscription = cx.subscribe_in(&auth_tab, window, Self::on_auth_tab_event);
        RequestFields {
            url_bar,
            params,
            headers,
            body_tab,
            auth_tab,
            _url_bar_subscription,
            _params_subscription,
            _headers_subscription,
            _body_tab_subscription,
            _auth_tab_subscription,
        }
    }

    /// An editor for `request`, with its method and URL already in the bar. The request
    /// starts clean: it is dirty once the bar applies an edit.
    pub fn new(request: HttpRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let RequestFields {
            url_bar,
            params,
            headers,
            body_tab,
            auth_tab,
            _url_bar_subscription,
            _params_subscription,
            _headers_subscription,
            _body_tab_subscription,
            _auth_tab_subscription,
        } = Self::build_request_fields(&request, window, cx);
        let settings_tab = cx.new(|cx| SettingsTab::new(&request, window, cx));
        let settings_tab_subscription =
            cx.subscribe_in(&settings_tab, window, Self::on_settings_tab_event);
        let tab_bar = cx.new(|cx| TabBar::new(cx).section());
        let tab_badges = request_tabs::TabBadges::of(&request);
        tab_bar.update(cx, |bar, cx| {
            bar.set_tabs(request_tabs::request_tabs(tab_badges), cx)
        });
        let tab_bar_subscription = cx.subscribe_in(&tab_bar, window, Self::on_tab_bar_event);
        let send_owner = http_engine::SendOwner::from(cx.entity_id());
        let release_subscription =
            cx.on_release(move |editor, cx| send::cancel_send_on_release(send_owner, editor, cx));
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            saved: request.clone(),
            draft: request,
            disk_change: None,
            dismissed_disk: None,
            file_gone: None,
            disk_watch: None,
            _disk_open_subscription: None,
            url_bar,
            tab_bar,
            active_tab: PARAMS_TAB_INDEX,
            tab_badges,
            variable_line: Vec::new(),
            environment_name: None,
            location: None,
            params,
            headers,
            body_tab,
            auth_tab,
            settings_tab,
            pre_import_request: None,
            pending_send: None,
            send_menu: None,
            inline_error: None,
            tls_offer_error: None,
            unresolved_warning: None,
            auth_warning: None,
            status_line: None,
            response_view: cx.new(ResponseView::new),
            split_focus: cx.focus_handle().tab_stop(true),
            split_fraction: response_split::DEFAULT_SPLIT_FRACTION,
            scopes: std::sync::Arc::new(variables::ScopeStack::new()),
            secret_sources: SecretSources::default(),
            history_origin: None,
            _url_bar_subscription,
            _params_subscription,
            _headers_subscription,
            _body_tab_subscription,
            _auth_tab_subscription,
            _settings_tab_subscription: settings_tab_subscription,
            _tab_bar_subscription: tab_bar_subscription,
            _release_subscription: release_subscription,
        }
    }

    /// A tab opened from History (REQ-HIST-02): `request` under a fresh id, remembering
    /// `recorded_id`, the id it was recorded under. `Decision:` the mark is set while the
    /// editor is built, so the scope wire's `observe_new` sees it and binds the tab to
    /// the window's open workspace as one of its unsaved tabs; `recorded_id` is how the
    /// wire tells a request that workspace holds from one recorded anywhere else.
    pub fn from_history(
        request: HttpRequest,
        recorded_id: ItemId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            history_origin: Some(recorded_id),
            ..Self::new(request, window, cx)
        }
    }

    /// The id a tab opened from History was recorded under; `None` for any other tab.
    pub fn history_origin(&self) -> Option<ItemId> {
        self.history_origin
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
        self.clear_stale_error();
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
        self.recheck_unresolved(cx);
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
        self.clear_stale_error();
        match event {
            HeadersEvent::RowsChanged => {
                self.draft.headers = self.headers.read(cx).rows(cx);
                self.copy_disabled_auto_headers_to_draft(cx);
            }
        }
        self.refresh_auto_headers(cx);
        self.recheck_unresolved(cx);
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
        self.clear_stale_error();
        match event {
            ParamsEvent::RowsChanged => {
                self.draft.params = self.params.read(cx).query_rows(cx);
                self.draft.path_variables = self.params.read(cx).path_rows(cx);
            }
        }
        self.refresh_auto_headers(cx);
        self.recheck_unresolved(cx);
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
        self.clear_stale_error();
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
        self.clear_stale_error();
        match event {
            BodyTabEvent::BodyChanged => self.draft.body = self.body_tab.read(cx).body(cx),
        }
        self.refresh_auto_headers(cx);
        self.recheck_unresolved(cx);
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
        self.clear_stale_error();
        match event {
            AuthTabEvent::AuthChanged => self.draft.auth = self.auth_tab.read(cx).auth(),
        }
        self.refresh_auto_headers(cx);
        self.recheck_unresolved(cx);
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

    /// The draft as its own [`serde_json`] document — the one serialization of the
    /// request there is, so a restored tab cannot drift from the model.
    fn serialize_draft(&self, _cx: &App) -> Option<String> {
        serde_json::to_string(&self.draft).ok()
    }

    /// The request's id: this tab's identity across restarts, so the tab comes back
    /// under the id its draft was saved under ([`restore_item_kind`], REQ-UI-02).
    fn persistent_id(&self, _cx: &App) -> Option<SharedString> {
        Some(self.draft.id.to_string().into())
    }
}

/// The item kind [`workspace::Restore`] rebuilds request tabs from (PRD REQ-UI-02,
/// issue #103): `pustman` registers it on the restore wire, so a request tab that was
/// open when a launch closed comes back as a request tab — not as the missing-items
/// toast a restore with no kind shows.
///
/// `Decision:` the persisted draft is the request's own [`serde_json`] document — the
/// exact value [`Item::serialize_draft`] writes, so there is one serialization and no
/// second draft format to keep in step. A draft that is missing, unreadable, or not the
/// request this id names is not claimed; `Restore` reports it as a missing item.
pub fn restore_item_kind() -> ItemFactory {
    Arc::new(
        |item_id: &str, draft: Option<String>, window: &mut Window, cx: &mut App| {
            let request = draft
                .as_deref()
                .and_then(|draft| serde_json::from_str::<HttpRequest>(draft).ok())
                .filter(|request| request.id.to_string() == item_id)?;
            let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
            Some(Box::new(editor))
        },
    )
}

impl Render for RequestEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The request side of the split: the URL bar and the tabs under it.
        let request_content = v_flex()
            .size_full()
            .min_w_0()
            .px(DynamicSpacing::Base12.rems(cx))
            .pt(DynamicSpacing::Base12.rems(cx))
            .gap(DynamicSpacing::Base06.rems(cx))
            // The changed-on-disk bar and its file-gone banner, above everything else
            // in the tab: they are about the tab's content as a whole (`disk_change`).
            .children(self.render_file_gone(cx))
            .children(self.render_disk_change(cx))
            // The pre-send unresolved-variable warning bar, above the URL bar
            // (REQ-VAR-07).
            .children(self.render_unresolved_warning(cx))
            // Where the request lives, with Save and Copy as cURL (`breadcrumb`).
            .child(self.render_breadcrumb(cx))
            // The URL bar, wrapped: the cURL paste is captured on this wrapper, which
            // is on the dispatch path exactly while the URL bar has focus (`curl_paste`).
            // Send ends the URL row; the URL bar's own errors stay under its field.
            .child(
                h_flex()
                    .items_start()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .min_w_0()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(DynamicSpacing::Base02.rems(cx))
                            .capture_action::<text_input::input::Paste>(
                                cx.listener(Self::handle_paste_curl),
                            )
                            .child(self.url_bar.clone())
                            // Under the field, so Tab reaches Undo before Send.
                            .children(self.render_import_undo(cx)),
                    )
                    .child(self.render_send_controls(cx)),
            )
            // Under the URL row, where the send happened: the inline failure.
            .children(self.render_send_error(cx))
            // And the hint when that send went out without its credentials.
            .children(self.render_auth_warning(cx))
            // The request tab strip over the tab content: the selection decides which
            // tab's content shows below.
            .child(self.render_tab_strip(cx))
            // Where each `{{variable}}` resolves from (`variable_line`), between the strip
            // and the tab's table: always on screen, never under a tall table.
            .when(self.active_tab != SETTINGS_TAB_INDEX, |this| {
                this.children(self.render_variable_line(cx))
            })
            .child(
                v_flex()
                    .debug_selector(|| REQUEST_TAB_CONTENT_SELECTOR.to_owned())
                    .pb(DynamicSpacing::Base08.rems(cx))
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .map(|this| {
                        // Compare's side-by-side field diff takes the content region
                        // while it is open (`disk_change`).
                        if let Some(compare) = self.render_compare(cx) {
                            this.child(compare)
                        } else {
                            let tab: gpui::AnyView = match self.active_tab {
                                HEADERS_TAB_INDEX => self.headers.clone().into(),
                                BODY_TAB_INDEX => self.body_tab.clone().into(),
                                AUTH_TAB_INDEX => self.auth_tab.clone().into(),
                                SETTINGS_TAB_INDEX => self.settings_tab.clone().into(),
                                _ => self.params.clone().into(),
                            };
                            this.child(tab)
                        }
                    }),
            )
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

/// A section title inside a request tab ("Query Params", "Headers"): the design
/// system's section-title style — [`LabelSize::Large`] (the pack's 16/24 section titles,
/// `docs/postman_redesign_pack/01_specs/design_system.md`) in the primary text colour —
/// so it stands one step above the 12 px column headers instead of looking like one
/// (#222). One implementation for every tab (DRY).
pub(crate) fn section_title(title: &'static str, selector: &'static str) -> impl IntoElement {
    div()
        .debug_selector(|| String::from(selector))
        .child(Label::new(SharedString::new_static(title)).size(LabelSize::Large))
}

/// Wire the request editor into the app: the app-level handlers behind
/// `workspace::NewRequest` and `request::FocusUrlBar` (`new_request`), and the
/// `workspace::Save` handler that writes the active request to its file (`save`).
pub fn init(cx: &mut App) {
    new_request::init(cx);
    read_only_wiring::init(cx);
    save::init(cx);
    disk_change::init(cx);
    breadcrumb::init(cx);
}

#[cfg(test)]
#[path = "request_editor_auto_header_tests.rs"]
mod auto_header_tests;

#[cfg(test)]
#[path = "request_editor_curl_reseed_tests.rs"]
mod curl_reseed_tests;

#[cfg(test)]
#[path = "request_editor_edit_tests.rs"]
mod edit_tests;

#[cfg(test)]
#[path = "m1_layout_fixes_tests.rs"]
mod m1_layout_fixes_tests;

#[cfg(test)]
#[path = "editor_layout_polish_tests.rs"]
mod editor_layout_polish_tests;

#[cfg(test)]
#[path = "section_title_tests.rs"]
mod section_title_tests;

#[cfg(test)]
#[path = "save_order_tests.rs"]
mod save_order_tests;

#[cfg(test)]
#[path = "file_follow_tests.rs"]
mod file_follow_tests;

#[cfg(test)]
#[path = "disk_change_fix_tests.rs"]
mod disk_change_fix_tests;
