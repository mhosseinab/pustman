//! The Params tab (REQ-HTTP-02): the query table and the path-variable table, kept in
//! sync with the URL bar in both directions.
//!
//! `Decision:` the raw URL text is the query's wire form — it is split on the first `?`,
//! then `&`, then the first `=`, with every piece kept verbatim (no `url::Url` parsing,
//! no percent-decoding: URLs hold `{{var}}` and may be relative or invalid, and
//! `{{var}}` must round-trip). The `#fragment` tail sits after the query and is carried
//! through untouched. Disabled rows are omitted from the URL but kept in the table, and
//! so are rows that say nothing about the request — no key and no value, the empty row
//! [`KeyValueTable::add_row`] seeds (#139): once any row is edited they must not
//! decorate the URL with a trailing `&` or a bare `?`.

use std::sync::Arc;

use api_model::{HttpRequest, KeyValue};
use gpui::{
    App, Context, ElementId, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Subscription, Window,
};
use ui::prelude::*;
use variables::ScopeStack;

use crate::key_value_table::{KeyValueTable, KeyValueTableEvent, KeyValueTableShape};
use crate::section_title;
use crate::url_bar::UrlBar;
use crate::variable_highlight;

/// The debug key the Path Variables section paints under — the test hook that lets a
/// test see whether the section is on screen at all (`VisualTestContext::debug_bounds`,
/// the `crate::send`'s selector pattern).
pub(crate) const PATH_SECTION_SELECTOR: &str = "params-path-variables-section";

/// The debug keys the tab's section titles paint under — the test hook that lets a test
/// measure them against the column headers they must stand above (#222).
pub(crate) const QUERY_SECTION_TITLE_SELECTOR: &str = "params-query-section-title";
pub(crate) const PATH_SECTION_TITLE_SELECTOR: &str = "params-path-section-title";

/// What a [`Params`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamsEvent {
    /// A row changed in either table; [`Params::query_rows`] and [`Params::path_rows`]
    /// are the new truth for the request draft.
    RowsChanged,
}

impl EventEmitter<ParamsEvent> for Params {}

/// The Params tab: the query rows under the URL, the `:name` path-variable rows under
/// them, and the two-way sync with the URL bar.
pub struct Params {
    url_bar: Entity<UrlBar>,
    query_table: Entity<KeyValueTable>,
    path_table: Entity<KeyValueTable>,
    /// The scopes both tables highlight their `{{variables}}` against (REQ-VAR-03).
    scopes: Arc<ScopeStack>,
    _query_subscription: Subscription,
    _path_subscription: Subscription,
}

impl Params {
    /// The tab for `request`: query rows seeded from the request's params (they carry
    /// description, enabled and `extra` the URL cannot) or, when it has none, from the
    /// URL's query; path rows from the URL's `:name` segments with the values the
    /// request stores for them.
    pub fn new(
        request: &HttpRequest,
        url_bar: Entity<UrlBar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query_table = cx.new(|cx| KeyValueTable::new(KeyValueTableShape::Editable, cx));
        let path_table = cx.new(|cx| KeyValueTable::new(KeyValueTableShape::Derived, cx));
        let query_rows = if request.params.is_empty() {
            let (_, query, _) = split_url(&request.url);
            query.map(parse_query).unwrap_or_default()
        } else {
            request.params.clone()
        };
        query_table.update(cx, |table, cx| table.set_rows(query_rows, window, cx));
        let path_rows = path_names(&request.url)
            .into_iter()
            .map(|name| stored_path_row(&name, &request.path_variables))
            .collect();
        path_table.update(cx, |table, cx| table.set_rows(path_rows, window, cx));
        let _query_subscription = cx.subscribe_in(&query_table, window, Self::on_query_table_event);
        let _path_subscription = cx.subscribe_in(&path_table, window, Self::on_path_table_event);
        let params = Self {
            url_bar,
            query_table,
            path_table,
            scopes: Arc::new(ScopeStack::new()),
            _query_subscription,
            _path_subscription,
        };
        params.refresh_highlights(cx);
        params
    }

    /// Store the scopes both tables highlight against and re-highlight them
    /// (REQ-VAR-03).
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.scopes = scopes;
        self.refresh_highlights(cx);
    }

    /// Re-highlight both tables against the current scopes.
    fn refresh_highlights(&self, cx: &mut App) {
        variable_highlight::apply_to_table(&self.query_table, &self.scopes, cx);
        variable_highlight::apply_to_table(&self.path_table, &self.scopes, cx);
    }

    /// The query rows as the request model stores them.
    pub fn query_rows(&self, cx: &App) -> Vec<KeyValue> {
        self.query_table.read(cx).rows()
    }

    /// The path-variable rows as the request model stores them.
    pub fn path_rows(&self, cx: &App) -> Vec<KeyValue> {
        self.path_table.read(cx).rows()
    }

    /// The query table — what tests and later tabs reach into.
    pub fn query_table(&self) -> &Entity<KeyValueTable> {
        &self.query_table
    }

    /// The path-variable table.
    pub fn path_table(&self) -> &Entity<KeyValueTable> {
        &self.path_table
    }

    /// The URL bar's text changed (typing or a `set_url` echo). The tables follow it —
    /// unless the URL is exactly what the rows already produce. That comparison is the
    /// loop guard: the URL bar echoes every `set_url` back as [`crate::url_bar::UrlBarEvent::UrlChanged`],
    /// and rebuilding the table for our own echo would recreate the rows out from under
    /// the caret in the cell being typed in and drop rows that are disabled in the table
    /// (they are kept in the table but omitted from the URL).
    pub fn url_changed(&self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (base, query, fragment) = split_url(url);
        let mut rows_changed = false;
        if build_url(base, &self.query_rows(cx), fragment) != url {
            let rows = query.map(parse_query).unwrap_or_default();
            self.query_table
                .update(cx, |table, cx| table.set_rows(rows, window, cx));
            rows_changed = true;
        }
        let names = path_names(base);
        if self
            .path_table
            .update(cx, |table, cx| table.sync_keys(&names, window, cx))
        {
            rows_changed = true;
        }
        if rows_changed {
            cx.emit(ParamsEvent::RowsChanged);
        }
        // Rows rewritten above are fresh inputs carrying text, and a programmatic
        // table write emits nothing, so they are highlighted here.
        self.refresh_highlights(cx);
    }

    /// The query rows changed: rebuild the URL from them and push it into the URL bar
    /// (the echo comes back through [`Self::url_changed`] and stops at the guard).
    fn on_query_table_event(
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
        let current = self.url_bar.read(cx).url(cx);
        let (base, _, fragment) = split_url(&current);
        let rebuilt = build_url(base, &self.query_rows(cx), fragment);
        if rebuilt != current {
            self.url_bar.update(cx, |bar, cx| bar.set_url(&rebuilt, cx));
        }
        cx.emit(ParamsEvent::RowsChanged);
    }

    /// The path rows changed. `Decision:` a path-variable row never writes the URL — its
    /// value substitutes at send time (REQ-EXEC), so editing it leaves the URL alone.
    fn on_path_table_event(
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
        cx.emit(ParamsEvent::RowsChanged);
    }

    /// The Path Variables section — shown only when the URL carries `:name` segments
    /// (Postman's behaviour, #221): a path variable exists because the URL names one,
    /// so an empty section would be noise. The table's rows are exactly those names
    /// ([`Self::url_changed`] keeps it in step with them).
    fn path_section(&self, cx: &App) -> Option<impl IntoElement> {
        let url = self.url_bar.read(cx).url(cx);
        if path_names(split_url(&url).0).is_empty() {
            return None;
        }
        Some(
            v_flex()
                .id(ElementId::Name(SharedString::new_static(
                    "path-variables-section",
                )))
                .debug_selector(|| String::from(PATH_SECTION_SELECTOR))
                .gap(DynamicSpacing::Base04.rems(cx))
                .child(section_title("Path Variables", PATH_SECTION_TITLE_SELECTOR))
                .child(self.path_table.clone()),
        )
    }
}

impl Render for Params {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(ElementId::Name(SharedString::new_static("params-tab")))
            .size_full()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(section_title("Query Params", QUERY_SECTION_TITLE_SELECTOR))
            .child(self.query_table.clone())
            .children(self.path_section(cx))
    }
}

/// The raw URL split into the part before `?`, the query without its `?`, and the
/// fragment tail without its `#` — every piece verbatim.
fn split_url(url: &str) -> (&str, Option<&str>, Option<&str>) {
    let (before_fragment, fragment) = match url.split_once('#') {
        Some((before, fragment)) => (before, Some(fragment)),
        None => (url, None),
    };
    let (base, query) = match before_fragment.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (before_fragment, None),
    };
    (base, query, fragment)
}

/// Parse a raw query into rows: split on `&`, then each segment on its first `=`, text
/// kept verbatim. An empty segment is a keyless row, so the table shows exactly what
/// the URL carries and nothing typed is lost on the way in — but [`build_url`] omits
/// those rows again (they say nothing about the request), so `?a=1&` rebuilds as
/// `?a=1` and the URL bar is rewritten only by a table edit, never under its own caret.
fn parse_query(query: &str) -> Vec<KeyValue> {
    query
        .split('&')
        .map(|segment| {
            let (key, value) = match segment.split_once('=') {
                Some((key, value)) => (key, Some(value.to_string())),
                None => (segment, None),
            };
            KeyValue {
                key: key.to_string(),
                value,
                description: None,
                enabled: true,
                extra: Default::default(),
            }
        })
        .collect()
}

/// The `:name` path segments of `base`, in order: every `/`-separated segment that
/// starts with `:`. A bare `:` is not a name.
fn path_names(base: &str) -> Vec<String> {
    base.split('/')
        .filter_map(|segment| segment.strip_prefix(':'))
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// Join `rows` back into a raw URL over `base`: the rows that say something about the
/// request — enabled and not the untouched empty row — in order, `&`-joined verbatim,
/// with `?` when there is at least one such row and `#fragment` at the end. A row with
/// a key but no value (`?a`) is a payload and keeps its bare key (#139).
fn build_url(base: &str, rows: &[KeyValue], fragment: Option<&str>) -> String {
    let enabled: Vec<&KeyValue> = rows
        .iter()
        .filter(|row| row.enabled && !(row.key.is_empty() && row.value.is_none()))
        .collect();
    let mut url = String::from(base);
    if !enabled.is_empty() {
        url.push('?');
        for (index, row) in enabled.iter().enumerate() {
            if index > 0 {
                url.push('&');
            }
            url.push_str(&row.key);
            if let Some(value) = &row.value {
                url.push('=');
                url.push_str(value);
            }
        }
    }
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }
    url
}

/// A path-variable row for `name`: the name from the URL, the value the request already
/// stores for that name if any.
fn stored_path_row(name: &str, stored: &[KeyValue]) -> KeyValue {
    let mut row = stored
        .iter()
        .find(|row| row.key == name)
        .cloned()
        .unwrap_or_else(|| KeyValue {
            key: name.to_string(),
            value: None,
            description: None,
            enabled: true,
            extra: Default::default(),
        });
    row.key = name.to_string();
    row
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "params_path_section_tests.rs"]
mod path_section_tests;

#[cfg(test)]
#[path = "params_url_build_tests.rs"]
mod url_build_tests;
