//! The Headers tab (REQ-HTTP-03): the user's header rows on the reusable
//! [`KeyValueTable`], header-name and header-value autocomplete from static lists of
//! common ones, and the request's auto headers listed with a send switch each — every
//! auto header can be disabled, and the disabled names live in the draft's
//! [`RequestSettings`].
//!
//! `Decision:` the auto rows are computed by `http_engine`'s own rules — a dry
//! [`prepare`] of the draft against an empty [`ScopeStack`] and a store that holds
//! nothing — so the editor never re-derives Content-Type/Content-Length/User-Agent
//! rules. `body::auto_headers` is crate-private there, and [`prepare`] is the narrowest
//! public API that fills [`http_engine::ResolvedRequest::auto_headers`] without a send.
//! With the empty scope stack no name is bound to a variable or a secret, so nothing
//! resolves and no secret is ever read; a draft whose URL does not parse (the user is
//! still typing it) lists no auto rows, because a dry [`prepare`] cannot produce them.
//!
//! `Decision:` the suggestion list renders under the header table, not anchored under
//! the exact key cell — anchoring needs cooperation from [`KeyValueTable`], which this
//! step does not edit. It is keyboard reachable from the key cell: Up/Down move the
//! selection and Escape dismisses as raw keystrokes (`on_key_down` below), while Enter
//! accepts through the captured [`text_input::input::Enter`] action (`handle_enter`) —
//! `text_input` binds Enter to that action, so the keystroke itself never bubbles to a
//! key listener, and the capture runs before the focused cell's own submit handler.
//!
//! `Decision:` the disabled auto headers live in the tab's [`RequestSettings`]
//! (`draft_settings`), the same "new truth for the request draft" role
//! [`Headers::rows`] has for the header rows. The one event this tab can emit is
//! matched exhaustively in `crate::request_editor`, which copies both — the rows and
//! the disabled-auto-header list — into the request draft (the write-through), and
//! pushes every draft edit back here through [`Headers::draft_changed`], which
//! re-seeds the list from the draft. Either direction lives in one place, so the tab
//! and the draft cannot drift: a draft replaced wholesale never keeps the switches of
//! the request it replaced.

use std::sync::Arc;

use api_model::{HttpRequest, KeyValue, RequestSettings};
use gpui::{
    App, Context, ElementId, Entity, EventEmitter, FocusHandle, IntoElement, KeyDownEvent,
    ParentElement, Render, Role, SharedString, Subscription, WeakEntity, Window,
};
use http_engine::prepare;
use secrets::SecretKey;
use ui::prelude::*;
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::key_value_table::{KeyValueTable, KeyValueTableEvent, KeyValueTableShape, RowField};
use crate::section_title;
use crate::variable_highlight;
use crate::variable_hover::NoStoredSecrets;

/// The debug keys the tab's section titles paint under — the test hook that lets a test
/// measure them against the column headers they must stand above (#222).
pub(crate) const HEADERS_SECTION_TITLE_SELECTOR: &str = "headers-section-title";
pub(crate) const AUTO_HEADERS_SECTION_TITLE_SELECTOR: &str = "headers-auto-section-title";

/// Header names the key cells suggest, most common first (REQ-HTTP-03). A `const`
/// slice: the list is static, so its order never jitters and no map is rendered.
const COMMON_HEADER_NAMES: &[&str] = &[
    "Accept",
    "Accept-Encoding",
    "Accept-Language",
    "Authorization",
    "Cache-Control",
    "Connection",
    "Content-Disposition",
    "Content-Encoding",
    "Content-Length",
    "Content-Type",
    "Cookie",
    "Date",
    "Expect",
    "Host",
    "If-Match",
    "If-None-Match",
    "Origin",
    "Range",
    "Referer",
    "Retry-After",
    "Server",
    "Transfer-Encoding",
    "User-Agent",
    "X-Api-Key",
    "X-Request-Id",
];

/// How many suggestions the list shows at once.
const SUGGESTION_LIMIT: usize = 6;

/// Values the value cells suggest, by header name (REQ-HTTP-03) — what a user most
/// often types after the row's name, most common first. A `const` slice of pairs: the
/// lists are static, so their order never jitters and no map is rendered.
const COMMON_HEADER_VALUES: &[(&str, &[&str])] = &[
    (
        "Accept",
        &["*/*", "application/json", "application/xml", "text/html"],
    ),
    ("Accept-Encoding", &["gzip,deflate,br", "gzip", "identity"]),
    ("Authorization", &["Bearer ", "Basic "]),
    (
        "Cache-Control",
        &["no-cache", "no-store", "max-age=0", "private", "public"],
    ),
    ("Connection", &["keep-alive", "close"]),
    (
        "Content-Type",
        &[
            "application/json",
            "application/x-www-form-urlencoded",
            "application/xml",
            "multipart/form-data",
            "text/plain",
            "text/html",
            "application/octet-stream",
        ],
    ),
];

/// One header the request carries on its own: listed read-only under the user's rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoHeader {
    pub name: String,
    pub value: String,
}

impl AutoHeader {
    /// Where this value comes from — what the row's tooltip and accessible label say.
    fn source(&self) -> &'static str {
        match self.name.as_str() {
            "Content-Type" => "from the request body's mode",
            "Content-Length" => "from the encoded body's size",
            "Host" => "from the request URL",
            _ => "Pustman's HTTP client default",
        }
    }
}

/// What a [`Headers`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadersEvent {
    /// The tab's contents changed: [`Headers::rows`] and [`Headers::draft_settings`]
    /// are the new truth for the request draft.
    RowsChanged,
}

impl EventEmitter<HeadersEvent> for Headers {}

/// The Headers tab: the user's header rows, the suggestions under them, and the
/// request's own headers, each with its send switch.
pub struct Headers {
    table: Entity<KeyValueTable>,
    /// The request's auto headers as of the last draft change — never recomputed in
    /// `render`.
    auto_rows: Vec<AutoHeader>,
    /// The row whose cell is being typed into, and what its text matches.
    suggestion_row: Option<u64>,
    /// Whether the open list suggests a key (names) or a value (that row's values).
    suggestion_field: RowField,
    suggestions: Vec<&'static str>,
    selected_suggestion: usize,
    /// `(row id, key text, value text)` as of the last table event: which cell an edit
    /// landed in is found by diffing against this, so the suggestion list follows the
    /// caret's row without `KeyValueTable` naming it.
    known_rows: Vec<(u64, String, String)>,
    /// The draft's request settings this tab edits (REQ-HTTP-03): which auto headers
    /// the request leaves out of the send. Seeded — and re-seeded on every draft
    /// change — from the request, with the list owned here like the table owns the
    /// rows.
    draft_settings: RequestSettings,
    /// The read-only auto-header block: one focusable region over the rows' text — the
    /// switches are tab stops of their own (see [`ui::Toggle`]).
    auto_focus: FocusHandle,
    /// The scopes the rows highlight their `{{variables}}` against (REQ-VAR-03).
    scopes: Arc<ScopeStack>,
    _table_subscription: Subscription,
}

impl Headers {
    /// The tab for `request`: user rows seeded from its headers, auto rows from the
    /// send pipeline's own rules.
    pub fn new(request: &HttpRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = cx.new(|cx| KeyValueTable::new(KeyValueTableShape::Editable, cx));
        table.update(cx, |table, cx| {
            table.set_rows(request.headers.clone(), window, cx)
        });
        let subscription = cx.subscribe_in(&table, window, Self::on_table_event);
        let mut headers = Self {
            table,
            auto_rows: Vec::new(),
            suggestion_row: None,
            suggestion_field: RowField::Key,
            suggestions: Vec::new(),
            selected_suggestion: 0,
            known_rows: Vec::new(),
            draft_settings: settings_for(request),
            auto_focus: cx.focus_handle().tab_stop(true),
            scopes: Arc::new(ScopeStack::new()),
            _table_subscription: subscription,
        };
        headers.sync_row_snapshot(cx);
        headers.auto_rows = auto_headers_for(request);
        headers.refresh_highlights(cx);
        headers
    }

    /// Store the scopes the rows highlight against and re-highlight them (REQ-VAR-03).
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.scopes = scopes;
        self.refresh_highlights(cx);
    }

    /// Re-highlight the user rows' key and value cells against the current scopes.
    fn refresh_highlights(&self, cx: &mut Context<Self>) {
        variable_highlight::apply_to_table(&self.table, &self.scopes, cx);
    }

    /// The user rows as the request model stores them.
    pub fn rows(&self, cx: &App) -> Vec<KeyValue> {
        self.table.read(cx).rows()
    }

    /// The draft's request settings as this tab edits them — the new truth for
    /// [`RequestSettings::disabled_auto_headers`] (REQ-HTTP-03), the same role
    /// [`Self::rows`] has for the header rows.
    pub fn draft_settings(&self) -> &RequestSettings {
        &self.draft_settings
    }

    /// The request's auto headers, read-only — the switches under them edit
    /// [`Self::draft_settings`] instead.
    pub fn auto_rows(&self) -> &[AutoHeader] {
        &self.auto_rows
    }

    /// The user-header table — what tests and later tabs reach into.
    pub fn table(&self) -> &Entity<KeyValueTable> {
        &self.table
    }

    /// The draft changed (URL, body, or the rows themselves): the auto headers follow
    /// it — a user header of the same name suppresses an auto one — and the draft's
    /// settings re-seed this tab's, so a draft replaced wholesale never keeps the
    /// switches of the request it replaced (REQ-HTTP-03). Called by the owner on every
    /// draft edit; never from `render`. Re-seeding cannot undo the switch just
    /// flipped: the owner writes this tab's list into the draft before pushing the
    /// draft back here.
    pub fn draft_changed(&mut self, request: &HttpRequest, cx: &mut Context<Self>) {
        self.auto_rows = auto_headers_for(request);
        self.draft_settings = settings_for(request);
        cx.notify();
    }

    /// Switch one auto header on or off for the draft (REQ-HTTP-03): a disabled auto
    /// header is not sent. The names compare ASCII-case-insensitively, like every
    /// header name (RFC 9110 §5.1).
    fn set_auto_header_enabled(&mut self, name: &str, enabled: bool, cx: &mut Context<Self>) {
        let disabled = &mut self.draft_settings.disabled_auto_headers;
        let listed = disabled
            .iter()
            .position(|entry| entry.eq_ignore_ascii_case(name));
        match (enabled, listed) {
            (false, None) => disabled.push(name.to_string()),
            (true, Some(index)) => {
                disabled.remove(index);
            }
            _ => {}
        }
        cx.emit(HeadersEvent::RowsChanged);
        cx.notify();
    }

    /// A user row changed: follow which key cell the edit landed in, and hand the new
    /// rows to the owner.
    fn on_table_event(
        &mut self,
        _table: &Entity<KeyValueTable>,
        event: &KeyValueTableEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != KeyValueTableEvent::RowsChanged {
            return;
        }
        self.refresh_highlights(cx);
        self.refresh_suggestions(cx);
        cx.emit(HeadersEvent::RowsChanged);
    }

    /// The key cell being typed into drives the suggestion list. Up/Down move the
    /// selection and Escape dismisses — with the list closed these keystrokes are not
    /// this tab's to consume and keep bubbling. Enter arrives as an action instead and
    /// is handled by [`Self::handle_enter`]; its arm here covers a raw Enter that
    /// bubbles.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.suggestions.is_empty() {
            return;
        }
        let count = self.suggestions.len();
        match event.keystroke.key.as_str() {
            "up" => {
                self.selected_suggestion = (self.selected_suggestion + count - 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            "down" => {
                self.selected_suggestion = (self.selected_suggestion + 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            "enter" => {
                cx.stop_propagation();
                self.accept_suggestion(window, cx);
            }
            "escape" => {
                cx.stop_propagation();
                self.dismiss_suggestions(cx);
            }
            _ => {}
        }
    }

    /// `Enter` while a suggestion list is open accepts it (REQ-HTTP-03). The keystroke
    /// is captured here — before the focused cell's own submit handler sees it —
    /// because `text_input` binds Enter to its [`text_input::input::Enter`] action and
    /// the raw keystroke never reaches [`Self::on_key_down`]. With no list open this
    /// does nothing and the action keeps bubbling: Enter still submits the cell.
    fn handle_enter(
        &mut self,
        _: &text_input::input::Enter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.suggestions.is_empty() {
            return;
        }
        cx.stop_propagation();
        self.accept_suggestion(window, cx);
    }

    /// Follow the table's rows to the cell being typed into and its suggestions: a key
    /// edit suggests header names, a value edit suggests values for that row's header
    /// (REQ-HTTP-03). A row appearing or leaving closes the list.
    fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let (ids, rows) = {
            let table = self.table.read(cx);
            (table.row_ids(), table.rows())
        };
        let current: Vec<(u64, String, String)> = ids
            .into_iter()
            .zip(rows)
            .map(|(id, row)| (id, row.key, row.value.unwrap_or_default()))
            .collect();
        let edited = current.iter().find_map(|(id, key, value)| {
            let known = self
                .known_rows
                .iter()
                .find(|(known_id, _, _)| known_id == id);
            match known {
                Some((_, known_key, _)) if known_key != key => {
                    Some((*id, RowField::Key, key.clone(), key.clone()))
                }
                Some((_, _, known_value)) if known_value != value => {
                    Some((*id, RowField::Value, key.clone(), value.clone()))
                }
                _ => None,
            }
        });
        self.known_rows = current;
        self.suggestion_field = RowField::Key;
        match edited {
            Some((id, field, key, typed)) if !typed.is_empty() => {
                self.suggestion_row = Some(id);
                self.suggestion_field = field;
                self.suggestions = match field {
                    RowField::Key => matching_header_names(&typed),
                    _ => matching_header_values(&key, &typed),
                };
                self.selected_suggestion = 0;
            }
            _ => {
                self.suggestion_row = None;
                self.suggestions.clear();
            }
        }
        cx.notify();
    }

    /// Replace the tracked row's key or value with the selected suggestion and put the
    /// caret back in that cell. The rows are rewritten wholesale (the table's own
    /// `set_rows` path), so the owner hears `RowsChanged` from here — a programmatic
    /// table write emits nothing by design.
    fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(row_id), Some(name), field) = (
            self.suggestion_row,
            self.suggestions.get(self.selected_suggestion).copied(),
            self.suggestion_field,
        ) else {
            return;
        };
        let ids = self.table.read(cx).row_ids();
        let Some(index) = ids.iter().position(|id| *id == row_id) else {
            self.dismiss_suggestions(cx);
            return;
        };
        let mut rows = self.table.read(cx).rows();
        let Some(row) = rows.get_mut(index) else {
            self.dismiss_suggestions(cx);
            return;
        };
        match field {
            RowField::Key => row.key = name.to_string(),
            _ => row.value = Some(name.to_string()),
        }
        self.table
            .update(cx, |table, cx| table.set_rows(rows, window, cx));
        // The rewrite built fresh cells carrying text, and a programmatic table write
        // emits nothing, so they are highlighted here.
        self.refresh_highlights(cx);
        self.dismiss_suggestions(cx);
        self.sync_row_snapshot(cx);
        let focus_id = self.table.read(cx).row_ids().get(index).copied();
        if let Some(focus_id) = focus_id {
            self.table.update(cx, |table, cx| {
                table.focus_cell(focus_id, field, window, cx)
            });
        }
        cx.emit(HeadersEvent::RowsChanged);
    }

    /// Close the suggestion list.
    fn dismiss_suggestions(&mut self, cx: &mut Context<Self>) {
        self.suggestion_row = None;
        self.suggestion_field = RowField::Key;
        self.suggestions.clear();
        self.selected_suggestion = 0;
        cx.notify();
    }

    /// Record the rows as they stand now, without touching the suggestion list (the
    /// close of an accepted suggestion).
    fn sync_row_snapshot(&mut self, cx: &App) {
        let (ids, rows) = {
            let table = self.table.read(cx);
            (table.row_ids(), table.rows())
        };
        self.known_rows = ids
            .into_iter()
            .zip(rows)
            .map(|(id, row)| (id, row.key, row.value.unwrap_or_default()))
            .collect();
    }

    /// The suggestion list under the table — empty when nothing is being suggested.
    /// The list labels follow what it suggests: names for a key cell, the row's values
    /// for a value cell.
    fn render_suggestions(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.suggestions.is_empty() {
            return None;
        }
        let (list_id, list_label, item_label) = match self.suggestion_field {
            RowField::Key => (
                "header-name-suggestions",
                "Header name suggestions",
                "Use header name",
            ),
            _ => (
                "header-value-suggestions",
                "Header value suggestions",
                "Use header value",
            ),
        };
        let selected = self.selected_suggestion;
        let weak = cx.weak_entity();
        let rows = self.suggestions.iter().enumerate().map(|(index, name)| {
            let chosen = *name;
            let weak = weak.clone();
            h_flex()
                .id(ElementId::Name(SharedString::from(format!(
                    "suggestion-{chosen}"
                ))))
                .debug_selector(|| format!("suggestion-{chosen}"))
                .role(Role::ListItem)
                .aria_label(format!("{item_label} {chosen}"))
                .items_center()
                .gap(DynamicSpacing::Base02.rems(cx))
                .px(DynamicSpacing::Base04.rems(cx))
                .py(DynamicSpacing::Base01.rems(cx))
                .map(|this| {
                    if index == selected {
                        this.bg(cx.theme().colors().element_background)
                    } else {
                        this
                    }
                })
                .hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
                .cursor_pointer()
                .on_click(move |_event, window, cx| {
                    if weak
                        .update(cx, |headers: &mut Headers, cx| {
                            headers.selected_suggestion = index;
                            headers.accept_suggestion(window, cx);
                        })
                        .is_err()
                    {
                        // Handled: the tab closed with its window; no row to edit.
                    }
                })
                .child(Label::new(SharedString::new_static(chosen)).size(LabelSize::Small))
        });
        Some(
            v_flex()
                .id(ElementId::Name(SharedString::new_static(list_id)))
                .debug_selector(|| String::from(list_id))
                .role(Role::List)
                .aria_label(list_label)
                .border_1()
                .border_color(cx.theme().colors().border)
                .rounded_md()
                .bg(cx.theme().colors().elevated_surface_background)
                .children(rows)
                .child(
                    Label::new(SharedString::new_static(
                        "Up/Down choose - Enter accepts - Escape dismisses",
                    ))
                    .size(LabelSize::XSmall)
                    .color(Self::suggestion_hint_color()),
                ),
        )
    }

    /// The colour the keystroke hint under the suggestion list draws in: one step back
    /// from the suggestions themselves (S03/R03 — it is secondary metadata, and
    /// `references/design_system.md` maps that to the muted text token). `Decision:`
    /// asserted through this accessor — the value `render` hands the label — because
    /// GPUI's test support exposes layout and painted quads but not text colours, the
    /// reason `crate::url_bar_method_color_tests` records for the same shape (#152).
    pub fn suggestion_hint_color() -> Color {
        Color::Muted
    }

    /// One auto header: its send switch (REQ-HTTP-03 — each auto header can be
    /// disabled), greyed, badged "auto", and labelled with where its value comes from.
    /// Nothing here is a text field, so there is nothing to type into.
    fn render_auto_row(
        row: &AutoHeader,
        enabled: bool,
        weak: WeakEntity<Headers>,
        cx: &App,
    ) -> impl IntoElement {
        let name = row.name.clone();
        let value = row.value.clone();
        let send_note = if enabled {
            "Sent automatically; switch it off to leave it out of the send."
        } else {
            "Not sent: this request leaves this header out; switch it on to send it."
        };
        let explanation = format!("{name}: {value} - {}. {}", row.source(), send_note);
        let toggle_name = name.clone();
        h_flex()
            .id(ElementId::Name(SharedString::from(format!(
                "auto-row-{name}"
            ))))
            .debug_selector(|| format!("auto-row-{name}"))
            .role(Role::ListItem)
            .aria_label(explanation.clone())
            .tooltip(Tooltip::text(explanation).builder())
            .items_center()
            .gap(DynamicSpacing::Base02.rems(cx))
            .min_w_0()
            .child(
                h_flex()
                    .id(ElementId::Name(SharedString::from(format!(
                        "auto-toggle-{name}"
                    ))))
                    .debug_selector(|| format!("auto-toggle-{name}"))
                    .child(
                        Toggle::new(
                            ElementId::Name(SharedString::from(format!(
                                "auto-header-{toggle_name}"
                            ))),
                            SharedString::from(format!("Send {toggle_name}")),
                            ToggleState::from(enabled),
                        )
                        .color(Color::Muted)
                        .on_change(move |state, _window, cx| {
                            if weak
                                .update(cx, |headers: &mut Headers, cx| {
                                    headers.set_auto_header_enabled(
                                        &toggle_name,
                                        state.is_selected(),
                                        cx,
                                    );
                                })
                                .is_err()
                            {
                                // Handled: the tab closed with its window; no setting to change.
                            }
                        }),
                    ),
            )
            .child(
                Label::new(value)
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .truncate(),
            )
            .child(
                div()
                    .debug_selector(|| format!("auto-badge-{name}"))
                    .child(Badge::new("auto").color(Color::Muted)),
            )
    }

    /// The block of auto headers: one focusable region over the rows' text, with each
    /// row's send switch as its own tab stop.
    fn render_auto_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.auto_focus.clone();
        let weak = cx.weak_entity();
        let mut rows = Vec::with_capacity(self.auto_rows.len());
        for row in &self.auto_rows {
            let enabled = !self.draft_settings.is_auto_header_disabled(&row.name);
            rows.push(Self::render_auto_row(row, enabled, weak.clone(), cx));
        }
        v_flex()
            .id(ElementId::Name(SharedString::new_static("auto-headers")))
            .debug_selector(|| String::from("auto-headers"))
            .role(Role::Group)
            .aria_label("Automatic headers, sent by Pustman unless switched off")
            .track_focus(&focus_handle)
            .gap(DynamicSpacing::Base01.rems(cx))
            .min_w_0()
            .on_click(move |_event, window, cx| window.focus(&focus_handle, cx))
            .children(rows)
    }
}

impl Render for Headers {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(ElementId::Name(SharedString::new_static("headers-tab")))
            .size_full()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .on_key_down(cx.listener(Self::on_key_down))
            .capture_action::<text_input::input::Enter>(cx.listener(Self::handle_enter))
            .child(section_title("Headers", HEADERS_SECTION_TITLE_SELECTOR))
            .child(self.table.clone())
            .children(self.render_suggestions(cx))
            .child(section_title(
                "Automatic Headers",
                AUTO_HEADERS_SECTION_TITLE_SELECTOR,
            ))
            .child(self.render_auto_rows(cx))
    }
}

/// The common header names `typed` is a proper prefix of, in list order.
fn matching_header_names(typed: &str) -> Vec<&'static str> {
    let typed = typed.to_ascii_lowercase();
    COMMON_HEADER_NAMES
        .iter()
        .copied()
        .filter(|name| {
            let name = name.to_ascii_lowercase();
            name.starts_with(&typed) && name != typed
        })
        .take(SUGGESTION_LIMIT)
        .collect()
}

/// The common values of header `name` that `typed` is a proper prefix of, in list
/// order. A name no list knows suggests nothing.
fn matching_header_values(name: &str, typed: &str) -> Vec<&'static str> {
    let Some((_, values)) = COMMON_HEADER_VALUES
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
    else {
        return Vec::new();
    };
    let typed = typed.to_ascii_lowercase();
    values
        .iter()
        .copied()
        .filter(|value| {
            let value = value.to_ascii_lowercase();
            value.starts_with(&typed) && value != typed
        })
        .take(SUGGESTION_LIMIT)
        .collect()
}

/// The request's settings as this tab keeps them (REQ-HTTP-03): the draft is the
/// source of truth and "inherit the global settings" (`None`) reads as the empty
/// override. The one place the tab reads them — [`Headers::new`] seeds and
/// [`Headers::draft_changed`] re-seeds through it.
fn settings_for(request: &HttpRequest) -> RequestSettings {
    request.settings.clone().unwrap_or_default()
}

/// The request's auto headers, computed by `http_engine`'s own rules (REQ-HTTP-03).
///
/// A dry [`prepare`] against an empty [`ScopeStack`] and the shared [`NoStoredSecrets`]
/// (the `variable_hover` "nothing store" both call sites use, #193 — the preview must
/// never reach the OS keychain: a keychain read can prompt the user, macOS): nothing
/// resolves, no secret is read (no name is bound to one), and no send happens. When the
/// draft cannot be prepared at all — its URL does not parse, typically while the user
/// is typing it — the preview lists nothing; that is the explicit outcome here, not a
/// swallowed failure: the send reports that failure to the user when it happens.
fn auto_headers_for(request: &HttpRequest) -> Vec<AutoHeader> {
    // The preview lists every header the send pipeline adds when it is enabled: the
    // switches under the rows are what decides which ones go out (`draft_settings`),
    // and a switched-off row must stay listed or it could never be switched back on.
    let mut preview = request.clone();
    if let Some(settings) = &mut preview.settings {
        settings.disabled_auto_headers.clear();
    }
    let store = NoStoredSecrets;
    let secret_key = |_scope: ScopeKind, _name: &str| -> Option<SecretKey> { None };
    match prepare(
        &preview,
        &[],
        &ScopeStack::new(),
        &store,
        &secret_key,
        &mut DynamicValues::new(),
    ) {
        Ok(prepared) => prepared
            .resolved
            .auto_headers
            .into_iter()
            .map(|header| AutoHeader {
                name: header.name,
                value: header.value,
            })
            .collect(),
        Err(_unpreparable) => Vec::new(),
    }
}

#[cfg(test)]
#[path = "headers_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "headers_value_tests.rs"]
mod value_tests;

#[cfg(test)]
#[path = "headers_suggestion_tests.rs"]
mod suggestion_tests;

#[cfg(test)]
#[path = "headers_reseed_tests.rs"]
mod reseed_tests;
