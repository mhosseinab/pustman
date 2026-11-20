//! The Headers tab (REQ-HTTP-03): the user's header rows on the reusable
//! [`KeyValueTable`], header-name autocomplete from a static list of common names, and
//! the request's auto headers listed read-only with an "auto" badge.
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
//! selection, Enter accepts, Escape dismisses (`on_key_down` below; the key listener
//! runs after `text_input`'s own key bindings, which leave these keystrokes
//! un-consumed).

use api_model::{HttpRequest, KeyValue};
use gpui::{
    App, Context, ElementId, Entity, EventEmitter, FocusHandle, IntoElement, KeyDownEvent,
    ParentElement, Render, Role, SharedString, Subscription, Window,
};
use http_engine::prepare;
use secrets::{SecretError, SecretKey, SecretStore, SecretValue};
use ui::prelude::*;
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::key_value_table::{KeyValueTable, KeyValueTableEvent, KeyValueTableShape, RowField};

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
    /// A user row changed; [`Headers::rows`] is the new truth for the request draft.
    RowsChanged,
}

impl EventEmitter<HeadersEvent> for Headers {}

/// The Headers tab: the user's header rows, the name suggestions under them, and the
/// request's own headers listed read-only.
pub struct Headers {
    table: Entity<KeyValueTable>,
    /// The request's auto headers as of the last draft change — never recomputed in
    /// `render`.
    auto_rows: Vec<AutoHeader>,
    /// The row whose key cell is being typed into, and the names its text matches.
    suggestion_row: Option<u64>,
    suggestions: Vec<&'static str>,
    selected_suggestion: usize,
    /// `(row id, key text)` as of the last table event: which key cell an edit landed
    /// in is found by diffing against this, so the suggestion list follows the caret's
    /// row without `KeyValueTable` naming it.
    known_keys: Vec<(u64, String)>,
    /// The read-only auto-header block: one focusable region, because it holds nothing
    /// to type into — clicking or tabbing to it and typing changes nothing.
    auto_focus: FocusHandle,
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
            suggestions: Vec::new(),
            selected_suggestion: 0,
            known_keys: Vec::new(),
            auto_focus: cx.focus_handle().tab_stop(true),
            _table_subscription: subscription,
        };
        headers.sync_row_snapshot(cx);
        headers.auto_rows = auto_headers_for(request);
        headers
    }

    /// The user rows as the request model stores them.
    pub fn rows(&self, cx: &App) -> Vec<KeyValue> {
        self.table.read(cx).rows()
    }

    /// The request's auto headers, read-only.
    pub fn auto_rows(&self) -> &[AutoHeader] {
        &self.auto_rows
    }

    /// The user-header table — what tests and later tabs reach into.
    pub fn table(&self) -> &Entity<KeyValueTable> {
        &self.table
    }

    /// The draft changed (URL, body, or the rows themselves): the auto headers follow
    /// it — a user header of the same name suppresses an auto one. Called by the owner
    /// on every draft edit; never from `render`.
    pub fn draft_changed(&mut self, request: &HttpRequest, cx: &mut Context<Self>) {
        self.auto_rows = auto_headers_for(request);
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
        self.refresh_suggestions(cx);
        cx.emit(HeadersEvent::RowsChanged);
    }

    /// The key cell being typed into drives the suggestion list. Up/Down move the
    /// selection, Enter accepts, Escape dismisses — with the list closed these
    /// keystrokes are not this tab's to consume and keep bubbling.
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

    /// Follow the table's rows to the key cell being typed into and its name
    /// suggestions. A value edit (or a row appearing or leaving) closes the list: it
    /// suggests names, not values.
    fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let (ids, rows) = {
            let table = self.table.read(cx);
            (table.row_ids(), table.rows())
        };
        let current: Vec<(u64, String)> = ids
            .into_iter()
            .zip(rows.into_iter().map(|row| row.key))
            .collect();
        let edited = current.iter().find_map(|(id, key)| {
            let known = self
                .known_keys
                .iter()
                .find(|(known_id, _)| known_id == id)
                .map(|(_, known_key)| known_key);
            match known {
                Some(known_key) if known_key != key => Some((*id, key.clone())),
                _ => None,
            }
        });
        self.known_keys = current;
        match edited {
            Some((id, key)) if !key.is_empty() => {
                self.suggestion_row = Some(id);
                self.suggestions = matching_header_names(&key);
                self.selected_suggestion = 0;
            }
            _ => {
                self.suggestion_row = None;
                self.suggestions.clear();
            }
        }
        cx.notify();
    }

    /// Replace the tracked row's key with the selected suggestion and put the caret
    /// back in that key cell. The rows are rewritten wholesale (the table's own
    /// `set_rows` path), so the owner hears `RowsChanged` from here — a programmatic
    /// table write emits nothing by design.
    fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(row_id), Some(name)) = (
            self.suggestion_row,
            self.suggestions.get(self.selected_suggestion).copied(),
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
        row.key = name.to_string();
        self.table
            .update(cx, |table, cx| table.set_rows(rows, window, cx));
        self.dismiss_suggestions(cx);
        self.sync_row_snapshot(cx);
        let focus_id = self.table.read(cx).row_ids().get(index).copied();
        if let Some(focus_id) = focus_id {
            self.table.update(cx, |table, cx| {
                table.focus_cell(focus_id, RowField::Key, window, cx)
            });
        }
        cx.emit(HeadersEvent::RowsChanged);
    }

    /// Close the suggestion list.
    fn dismiss_suggestions(&mut self, cx: &mut Context<Self>) {
        self.suggestion_row = None;
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
        self.known_keys = ids
            .into_iter()
            .zip(rows.into_iter().map(|row| row.key))
            .collect();
    }

    /// The suggestion list under the table — empty when nothing is being suggested.
    fn render_suggestions(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.suggestions.is_empty() {
            return None;
        }
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
                .aria_label(format!("Use header name {chosen}"))
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
                .id(ElementId::Name(SharedString::new_static(
                    "header-name-suggestions",
                )))
                .debug_selector(|| String::from("header-name-suggestions"))
                .role(Role::List)
                .aria_label("Header name suggestions")
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
                    .color(Color::Muted),
                ),
        )
    }

    /// One auto header: greyed, badged "auto", and labelled with where its value comes
    /// from — nothing here is a text field, so there is nothing to type into.
    fn render_auto_row(row: &AutoHeader, cx: &App) -> impl IntoElement {
        let name = row.name.clone();
        let value = row.value.clone();
        let explanation = format!(
            "{name}: {value} - {}. Sent automatically; this row is read-only.",
            row.source()
        );
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
                Label::new(name.clone())
                    .size(LabelSize::Small)
                    .color(Color::Muted),
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

    /// The read-only block of auto headers: one focusable region over the rows.
    fn render_auto_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.auto_focus.clone();
        v_flex()
            .id(ElementId::Name(SharedString::new_static("auto-headers")))
            .debug_selector(|| String::from("auto-headers"))
            .role(Role::Group)
            .aria_label("Automatic headers, sent by Pustman and not editable")
            .track_focus(&focus_handle)
            .gap(DynamicSpacing::Base01.rems(cx))
            .min_w_0()
            .on_click(move |_event, window, cx| window.focus(&focus_handle, cx))
            .children(
                self.auto_rows
                    .iter()
                    .map(|row| Self::render_auto_row(row, cx)),
            )
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
            .child(section_label("Headers"))
            .child(self.table.clone())
            .children(self.render_suggestions(cx))
            .child(section_label("Automatic Headers"))
            .child(self.render_auto_rows(cx))
    }
}

/// A section label inside the tab.
fn section_label(title: &'static str) -> impl IntoElement {
    Label::new(SharedString::from(title))
        .size(LabelSize::Small)
        .color(Color::Muted)
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

/// The request's auto headers, computed by `http_engine`'s own rules (REQ-HTTP-03).
///
/// A dry [`prepare`] against an empty [`ScopeStack`] and [`NoStoredSecrets`]: nothing
/// resolves, no secret is read (no name is bound to one), and no send happens. When the
/// draft cannot be prepared at all — its URL does not parse, typically while the user
/// is typing it — the preview lists nothing; that is the explicit outcome here, not a
/// swallowed failure: the send reports that failure to the user when it happens.
fn auto_headers_for(request: &HttpRequest) -> Vec<AutoHeader> {
    let store = NoStoredSecrets;
    let secret_key = |_scope: ScopeKind, _name: &str| -> Option<SecretKey> { None };
    match prepare(
        request,
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

/// The secret store the auto-header preview resolves against: nothing is stored and
/// nothing can be. `secrets::MemorySecretStore` is compiled for tests only (behind the
/// crate's `test-support` feature), and the preview must never reach the OS keychain —
/// a keychain read can prompt the user (macOS). With the empty [`ScopeStack`] this
/// store is passed beside, [`prepare`] binds no name to any secret and never reads it.
struct NoStoredSecrets;

impl SecretStore for NoStoredSecrets {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        Err(SecretError::NotFound { key: key.clone() })
    }

    fn set(&self, _key: &SecretKey, _value: &SecretValue) -> Result<(), SecretError> {
        Err(SecretError::BackendUnavailable {
            reason: String::from("the auto-header preview is read-only and stores no secrets"),
        })
    }

    /// Deleting a key that has no secret succeeds ([`SecretStore`]'s own contract).
    fn delete(&self, _key: &SecretKey) -> Result<(), SecretError> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "headers_tests.rs"]
mod tests;
