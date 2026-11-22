//! The Auth tab (REQ-HTTP-06, REQ-MODEL-01, REQ-VAR-SHARE-03): the auth-type picker
//! and one editor per modelled type — inherit, none, Basic, Bearer and API key — plus
//! the source of the inherited auth (REQ-MODEL-01) and the nudge away from literal
//! secrets (REQ-VAR-SHARE-03). Digest, OAuth and the other kinds the model keeps raw
//! show read-only.
//!
//! `Decision:` the tab keeps each type's fields alive across type switches and hands
//! its owner only the active type's [`Auth`] ([`AuthTab::auth`]) — the `body_tab` rule:
//! switching Basic → Bearer → Basic restores the Basic fields, and the draft never
//! carries the leftovers of another type. Every modelled type also keeps the `extra`
//! its imported auth carried, so switching away and back round-trips an import
//! (REQ-MODEL-02).
//!
//! `Decision:` [`Auth::Unsupported`] (Digest, OAuth 2.0, …) shows read-only and is
//! handed back untouched until the user picks one of the five types — picking one
//! *is* editing the auth, and the unmodelled auth is then replaced, the rule S28a set
//! for [`Body::Unsupported`](api_model::Body::Unsupported).
//!
//! `Decision:` the parent chain arrives through [`AuthTab::set_parents`] as display
//! names plus each level's auth, nearest first; who supplies it at runtime (the
//! workspace knows the request's folder and collection) is a later wiring concern.
//! The label is computed with [`effective_auth`] over that chain — the model's own
//! walk, never a second one here. With no chain set it reads "No auth inherited".
//!
//! `Decision:` the three credential fields (Basic password, Bearer token, API key
//! value) run `text_input`'s masked mode (#178): they show one mask per character to
//! the line they paint and to the platform alike, never the value — S06 reads the
//! effective auth "without exposing credentials". Masking is display-deep:
//! [`AuthTab::auth`] still hands the draft the value. The fields a user names things
//! in (username, API key name) stay clear; secret reveal is a separate concern (S03)
//! and is not this mode.
//!
//! `Decision:` the REQ-VAR-SHARE-03 nudge is one per tab instance and non-blocking:
//! a single inline row of the tab — not a toast per keystroke, not a persistent
//! app-level warning — up while a secret field (Basic password, Bearer token, API key
//! value) holds literal text outside its `{{variable}}` references and down as soon as
//! every secret field holds only references again. "One-time" is read against the
//! instance, not as a shown-once latch: the transient text of typing `{{token}}` is
//! itself literal mid-typing, so a latch would be spent before the reference is
//! complete and the literal that really needs the nudge would never get it. The
//! nudge reacts only to edits — opening a request that already carries a literal is
//! not an edit. The references come from [`variables::spans`], the crate's one
//! placeholder scanner.
//!
//! `Decision:` the type and location pickers compose the `ui` [`Button`] and
//! [`ContextMenu`] in the shared [`floating_menu`] layer — the `body_tab` shape, whose
//! one-time copy of that helper this module used to carry is now hoisted into `ui` so
//! no picker keeps a copy of its own (#177).

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{ApiKeyLocation, Auth, AuthSource, HttpRequest, effective_auth};
use gpui::{
    AnyElement, App, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement, Render, SharedString, Subscription, Window,
};
use text_input::{TextInput, TextInputEvent};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;

use variables::{ScopeStack, spans};

use crate::variable_highlight;

/// The height of one control (a [`ButtonSize::Medium`]) in pixels at the default
/// scale: a picker menu is anchored just under it (`body_tab`).
const CONTROL_HEIGHT_PX: f32 = 28.;

/// The name column's width in pixels: fits "Username"/"Password" with room to spare.
const NAME_COLUMN_WIDTH_PX: f32 = 96.;

/// The form's widest extent in rems: past it a field only grows emptier.
const FORM_MAX_WIDTH_REMS: f32 = 40.;

/// Test hook: the inherited-source line under the type picker.
pub const INHERITED_SOURCE_SELECTOR: &str = "auth-inherited-source";

/// Test hook: the REQ-VAR-SHARE-03 hint under the type's fields.
pub const SECRET_HINT_SELECTOR: &str = "auth-secret-hint";

/// What an [`AuthTab`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthTabEvent {
    /// The tab's contents changed: [`AuthTab::auth`] is the new truth for the draft.
    AuthChanged,
}

impl EventEmitter<AuthTabEvent> for AuthTab {}

/// The auth types of REQ-HTTP-06 this tab edits, plus [`AuthType::Unsupported`] — an
/// auth no type here models (Digest, OAuth 2.0, …), shown read-only until one of the
/// five replaces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthType {
    Inherit,
    None,
    Basic,
    Bearer,
    ApiKey,
    Unsupported,
}

impl AuthType {
    /// The types the picker lists, in picker order. `Unsupported` is not one of them:
    /// once a type replaces an unmodelled auth there is no way back to it.
    pub const PICKER_TYPES: [AuthType; 5] = [
        AuthType::Inherit,
        AuthType::None,
        AuthType::Basic,
        AuthType::Bearer,
        AuthType::ApiKey,
    ];

    /// The type's name as the picker and the type button show it.
    pub fn label(self) -> &'static str {
        match self {
            AuthType::Inherit => "Inherit auth from parent",
            AuthType::None => "No auth",
            AuthType::Basic => "Basic auth",
            AuthType::Bearer => "Bearer token",
            AuthType::ApiKey => "API key",
            AuthType::Unsupported => "Unsupported",
        }
    }
}

/// Which editable field an edit landed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthField {
    BasicUsername,
    BasicPassword,
    BearerToken,
    ApiKeyName,
    ApiKeyValue,
}

impl AuthField {
    /// Whether a value typed here is a credential: the one list behind both the
    /// REQ-VAR-SHARE-03 nudge and the masked fields (#178).
    fn is_secret(self) -> bool {
        matches!(
            self,
            AuthField::BasicPassword | AuthField::BearerToken | AuthField::ApiKeyValue
        )
    }
}

/// The Auth tab: the type picker, the active type's fields, and every type's state.
pub struct AuthTab {
    /// The parent chain, nearest first — `(display name, that level's auth)`.
    parents: Vec<(SharedString, Auth)>,
    /// Which type's editor shows and which [`Auth`] [`Self::auth`] reports.
    auth_type: AuthType,
    // Per-type state, kept across type switches (the `body_tab` rule).
    basic_username: Option<String>,
    basic_password: Option<String>,
    basic_extra: BTreeMap<String, serde_json::Value>,
    bearer_token: Option<String>,
    bearer_extra: BTreeMap<String, serde_json::Value>,
    api_key_name: Option<String>,
    api_key_value: Option<String>,
    api_key_location: Option<ApiKeyLocation>,
    api_key_extra: BTreeMap<String, serde_json::Value>,
    /// The unmodelled auth as imported — handed back untouched while it is the type.
    unsupported: Option<Auth>,
    // The stored fields' mirrors: created once and kept, so editing one keeps its
    // focus and caret (the `body_tab` form rows' rule). Canonical state is the fields
    // above; a programmatic fill echoes [`TextInputEvent::Changed`] and is compared
    // against them into a no-op.
    basic_username_input: Entity<TextInput>,
    basic_password_input: Entity<TextInput>,
    bearer_token_input: Entity<TextInput>,
    api_key_name_input: Entity<TextInput>,
    api_key_value_input: Entity<TextInput>,
    /// The REQ-VAR-SHARE-03 nudge: up while a secret field holds literal text.
    secret_hint: bool,
    /// The open picker menus and their subscriptions: dropped together when they
    /// dismiss.
    type_menu: Option<Entity<ContextMenu>>,
    type_menu_subscription: Option<Subscription>,
    location_menu: Option<Entity<ContextMenu>>,
    location_menu_subscription: Option<Subscription>,
    /// The type button's focus handle: the menu hands focus back to it when it closes.
    type_focus_handle: FocusHandle,
    /// The location button's focus handle, likewise.
    location_focus_handle: FocusHandle,
    /// The scopes the auth fields highlight their `{{variables}}` against (REQ-VAR-03).
    scopes: Arc<ScopeStack>,
    _field_subscriptions: Vec<Subscription>,
}

impl AuthTab {
    /// The tab for `request`: every type's state seeded from its auth, so the tab
    /// round-trips what the request already carries until the user edits it.
    pub fn new(request: &HttpRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let basic_username_input = make_input("Username", AuthField::BasicUsername, window, cx);
        let basic_password_input = make_input("Password", AuthField::BasicPassword, window, cx);
        let bearer_token_input = make_input("Token", AuthField::BearerToken, window, cx);
        let api_key_name_input = make_input("Key", AuthField::ApiKeyName, window, cx);
        let api_key_value_input = make_input("Value", AuthField::ApiKeyValue, window, cx);
        let mut tab = Self {
            parents: Vec::new(),
            auth_type: AuthType::None,
            basic_username: None,
            basic_password: None,
            basic_extra: BTreeMap::new(),
            bearer_token: None,
            bearer_extra: BTreeMap::new(),
            api_key_name: None,
            api_key_value: None,
            api_key_location: None,
            api_key_extra: BTreeMap::new(),
            unsupported: None,
            basic_username_input,
            basic_password_input,
            bearer_token_input,
            api_key_name_input,
            api_key_value_input,
            secret_hint: false,
            type_menu: None,
            type_menu_subscription: None,
            location_menu: None,
            location_menu_subscription: None,
            type_focus_handle: cx.focus_handle().tab_stop(true),
            location_focus_handle: cx.focus_handle().tab_stop(true),
            scopes: Arc::new(ScopeStack::new()),
            _field_subscriptions: Vec::new(),
        };
        // Seed before the fields are wired back to the tab, so the fills are not
        // edits: they neither announce themselves nor spend the secret nudge.
        tab.seed(&request.auth, cx);
        tab._field_subscriptions = [
            (AuthField::BasicUsername, &tab.basic_username_input),
            (AuthField::BasicPassword, &tab.basic_password_input),
            (AuthField::BearerToken, &tab.bearer_token_input),
            (AuthField::ApiKeyName, &tab.api_key_name_input),
            (AuthField::ApiKeyValue, &tab.api_key_value_input),
        ]
        .into_iter()
        .map(|(field, input)| {
            cx.subscribe_in(input, window, move |tab, _input, event, _window, cx| {
                tab.on_field_changed(field, event, cx);
            })
        })
        .collect();
        tab.refresh_highlights(cx);
        tab
    }

    /// Store the scopes the auth fields highlight against and re-highlight them
    /// (REQ-VAR-03).
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.scopes = scopes;
        self.refresh_highlights(cx);
    }

    /// Re-highlight every auth field against the current scopes.
    fn refresh_highlights(&self, cx: &mut Context<Self>) {
        for input in [
            &self.basic_username_input,
            &self.basic_password_input,
            &self.bearer_token_input,
            &self.api_key_name_input,
            &self.api_key_value_input,
        ] {
            variable_highlight::apply_to_input(input, &self.scopes, cx);
        }
    }

    /// The parent chain that resolves `Inherit` (REQ-MODEL-01), nearest first — what
    /// [`Self::inherited_source_label`] reads. The owner keeps this tab's chain across
    /// a whole-request replacement (`curl_paste`'s import).
    pub fn parents(&self) -> &[(SharedString, Auth)] {
        &self.parents
    }

    /// Set the parent chain with display names, nearest first — the workspace's job
    /// at runtime, a later wiring concern.
    pub fn set_parents(&mut self, parents: Vec<(SharedString, Auth)>, cx: &mut Context<Self>) {
        self.parents = parents;
        cx.notify();
    }

    /// The auth as the active type holds it — the new truth for the request draft.
    pub fn auth(&self) -> Auth {
        match self.auth_type {
            AuthType::Inherit => Auth::Inherit,
            AuthType::None => Auth::None,
            AuthType::Basic => Auth::Basic {
                username: self.basic_username.clone(),
                password: self.basic_password.clone(),
                extra: self.basic_extra.clone(),
            },
            AuthType::Bearer => Auth::Bearer {
                token: self.bearer_token.clone(),
                extra: self.bearer_extra.clone(),
            },
            AuthType::ApiKey => Auth::ApiKey {
                key: self.api_key_name.clone(),
                value: self.api_key_value.clone(),
                location: self.api_key_location,
                extra: self.api_key_extra.clone(),
            },
            AuthType::Unsupported => self.unsupported.clone().unwrap_or(Auth::Inherit),
        }
    }

    /// Switch to `auth_type` — the picker's entries run here. The state of the type
    /// left behind is kept; the draft gets only the type now active.
    pub fn set_auth_type(&mut self, auth_type: AuthType, cx: &mut Context<Self>) {
        if self.auth_type == auth_type {
            return;
        }
        self.auth_type = auth_type;
        cx.emit(AuthTabEvent::AuthChanged);
        cx.notify();
    }

    /// Put the API key in its header or its query string (REQ-HTTP-06) — the
    /// location picker's entries run here.
    pub fn set_api_key_location(&mut self, location: ApiKeyLocation, cx: &mut Context<Self>) {
        if self.api_key_location.unwrap_or(ApiKeyLocation::Header) == location {
            return;
        }
        self.api_key_location = Some(location);
        cx.emit(AuthTabEvent::AuthChanged);
        cx.notify();
    }

    /// Where the active auth comes from (REQ-MODEL-01), computed with the model's
    /// [`effective_auth`] over this request's auth and its parent chain. With no
    /// chain set there is nothing to inherit: "No auth inherited".
    pub fn inherited_source_label(&self) -> SharedString {
        if self.parents.is_empty() {
            return SharedString::new_static("No auth inherited");
        }
        let current = self.auth();
        let chain: Vec<&Auth> = std::iter::once(&current)
            .chain(self.parents.iter().map(|(_, auth)| auth))
            .collect();
        let (_, source) = effective_auth(&chain);
        match source {
            AuthSource::Request => SharedString::new_static("Set on this request"),
            AuthSource::Default => SharedString::new_static("No auth inherited"),
            AuthSource::Folder | AuthSource::Collection => {
                match self.deciding_parent_name(&chain) {
                    Some(name) => SharedString::from(format!("Inherited from {name}")),
                    None => SharedString::new_static("Inherited from a parent"),
                }
            }
        }
    }

    /// The display name of the level [`effective_auth`] decided from — found by
    /// asking it on growing prefixes of the chain, so the inheritance rules live only
    /// in the model even for "which level exactly".
    fn deciding_parent_name(&self, chain: &[&Auth]) -> Option<SharedString> {
        (1..chain.len())
            .find(|&end| effective_auth(&chain[..=end]).1 != AuthSource::Default)
            .and_then(|end| self.parents.get(end - 1))
            .map(|(name, _)| name.clone())
    }

    /// Whether the REQ-VAR-SHARE-03 nudge is up.
    pub fn shows_secret_hint(&self) -> bool {
        self.secret_hint
    }

    /// The Bearer token field — what tests and later steps reach into.
    pub fn bearer_token_input(&self) -> &Entity<TextInput> {
        &self.bearer_token_input
    }

    /// Seed every type's state from `auth` — the open path; a whole-request
    /// replacement rebuilds the tab instead (the `body_tab` rule).
    fn seed(&mut self, auth: &Auth, cx: &mut Context<Self>) {
        match auth {
            Auth::Inherit => self.auth_type = AuthType::Inherit,
            Auth::None => self.auth_type = AuthType::None,
            Auth::Basic {
                username,
                password,
                extra,
            } => {
                self.auth_type = AuthType::Basic;
                self.basic_username = username.clone();
                self.basic_password = password.clone();
                self.basic_extra = extra.clone();
                fill(&self.basic_username_input, username.as_deref(), cx);
                fill(&self.basic_password_input, password.as_deref(), cx);
            }
            Auth::Bearer { token, extra } => {
                self.auth_type = AuthType::Bearer;
                self.bearer_token = token.clone();
                self.bearer_extra = extra.clone();
                fill(&self.bearer_token_input, token.as_deref(), cx);
            }
            Auth::ApiKey {
                key,
                value,
                location,
                extra,
            } => {
                self.auth_type = AuthType::ApiKey;
                self.api_key_name = key.clone();
                self.api_key_value = value.clone();
                self.api_key_location = *location;
                self.api_key_extra = extra.clone();
                fill(&self.api_key_name_input, key.as_deref(), cx);
                fill(&self.api_key_value_input, value.as_deref(), cx);
            }
            Auth::Unsupported(_) => {
                self.auth_type = AuthType::Unsupported;
                self.unsupported = Some(auth.clone());
            }
        }
    }

    /// A field was edited (or its text was set programmatically). A programmatic fill
    /// echoes [`TextInputEvent::Changed`] with the text the tab already holds;
    /// comparing against the canonical value turns that echo into a no-op instead of
    /// a spurious change event.
    fn on_field_changed(
        &mut self,
        field: AuthField,
        event: &TextInputEvent,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Changed {
            return;
        }
        self.refresh_highlights(cx);
        let text = self.field_input(field).read(cx).text();
        let updated = (!text.is_empty()).then_some(text);
        if self.field_value(field) == updated.as_deref() {
            return;
        }
        self.set_field_value(field, updated);
        if field.is_secret() {
            self.update_secret_hint(cx);
        }
        cx.emit(AuthTabEvent::AuthChanged);
        cx.notify();
    }

    /// Re-evaluate the REQ-VAR-SHARE-03 nudge over every secret field: up while one
    /// of them holds literal text, down once they hold only variable references —
    /// one hint row per tab instance, never blocking an edit.
    fn update_secret_hint(&mut self, cx: &mut Context<Self>) {
        self.secret_hint = [
            self.basic_password.as_deref(),
            self.bearer_token.as_deref(),
            self.api_key_value.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(has_literal_secret);
        cx.notify();
    }

    /// The stored input behind `field`.
    fn field_input(&self, field: AuthField) -> &Entity<TextInput> {
        match field {
            AuthField::BasicUsername => &self.basic_username_input,
            AuthField::BasicPassword => &self.basic_password_input,
            AuthField::BearerToken => &self.bearer_token_input,
            AuthField::ApiKeyName => &self.api_key_name_input,
            AuthField::ApiKeyValue => &self.api_key_value_input,
        }
    }

    /// The canonical value behind `field` — what [`Self::auth`] packs into the model.
    fn field_value(&self, field: AuthField) -> Option<&str> {
        match field {
            AuthField::BasicUsername => self.basic_username.as_deref(),
            AuthField::BasicPassword => self.basic_password.as_deref(),
            AuthField::BearerToken => self.bearer_token.as_deref(),
            AuthField::ApiKeyName => self.api_key_name.as_deref(),
            AuthField::ApiKeyValue => self.api_key_value.as_deref(),
        }
    }

    /// Store an edited field. An empty field is `None` again — the model stores
    /// absence, not empty strings.
    fn set_field_value(&mut self, field: AuthField, value: Option<String>) {
        let slot = match field {
            AuthField::BasicUsername => &mut self.basic_username,
            AuthField::BasicPassword => &mut self.basic_password,
            AuthField::BearerToken => &mut self.bearer_token,
            AuthField::ApiKeyName => &mut self.api_key_name,
            AuthField::ApiKeyValue => &mut self.api_key_value,
        };
        *slot = value;
    }

    /// Open the auth-type menu under the type button.
    fn open_type_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.type_menu.is_some() {
            return;
        }
        let tab = cx.weak_entity();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx);
            for auth_type in AuthType::PICKER_TYPES {
                let tab = tab.clone();
                menu = menu.entry(auth_type.label(), move |_event, _window, cx| {
                    if tab
                        .update(cx, |tab, cx| tab.set_auth_type(auth_type, cx))
                        .is_err()
                    {
                        // The tab was dropped while the menu was open; there is no
                        // auth left to edit.
                    }
                });
            }
            menu
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_type_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.type_menu = Some(menu);
        self.type_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The type menu is done — an entry ran or Escape backed out.
    fn on_type_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.type_menu = None;
        self.type_menu_subscription = None;
        window.focus(&self.type_focus_handle, cx);
        cx.notify();
    }

    /// Open the API key's location menu under the location button.
    fn open_location_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.location_menu.is_some() {
            return;
        }
        let tab = cx.weak_entity();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx);
            for location in [ApiKeyLocation::Header, ApiKeyLocation::Query] {
                let tab = tab.clone();
                menu = menu.entry(location_label(location), move |_event, _window, cx| {
                    if tab
                        .update(cx, |tab, cx| tab.set_api_key_location(location, cx))
                        .is_err()
                    {
                        // The tab was dropped while the menu was open; there is no
                        // auth left to edit.
                    }
                });
            }
            menu
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_location_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.location_menu = Some(menu);
        self.location_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The location menu is done.
    fn on_location_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.location_menu = None;
        self.location_menu_subscription = None;
        window.focus(&self.location_focus_handle, cx);
        cx.notify();
    }

    /// The type control: the dropdown trigger. The menu floats under it.
    fn render_type_control(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .relative()
            .child(
                Button::new(
                    ElementId::from(&self.type_focus_handle),
                    self.type_button_label(),
                )
                .size(ButtonSize::Medium)
                .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
                // The tooltip hides while its own dropdown is open (#267): with the
                // pointer still on the anchor, the balloon would paint over the menu rows.
                .tooltip(Tooltip::text("Choose the auth type").hidden(self.type_menu.is_some()))
                .focus_handle(self.type_focus_handle.clone())
                .on_click(cx.listener(|this, _event, window, cx| this.open_type_menu(window, cx))),
            )
            .children(floating_menu(
                &self.type_menu,
                px_to_rems(CONTROL_HEIGHT_PX),
                cx,
            ))
            .into_any_element()
    }

    /// The location control of the API key type: the dropdown trigger, likewise.
    fn render_location_control(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .relative()
            .child(
                Button::new(
                    ElementId::from(&self.location_focus_handle),
                    SharedString::from(location_label(
                        self.api_key_location.unwrap_or(ApiKeyLocation::Header),
                    )),
                )
                .size(ButtonSize::Medium)
                .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
                // Likewise for the location dropdown (#267).
                .tooltip(
                    Tooltip::text("Where the API key is sent").hidden(self.location_menu.is_some()),
                )
                .focus_handle(self.location_focus_handle.clone())
                .on_click(
                    cx.listener(|this, _event, window, cx| this.open_location_menu(window, cx)),
                ),
            )
            .children(floating_menu(
                &self.location_menu,
                px_to_rems(CONTROL_HEIGHT_PX),
                cx,
            ))
            .into_any_element()
    }

    /// The type button's label: the active type's name, or the imported auth's own
    /// type name for a type no picker entry models.
    fn type_button_label(&self) -> SharedString {
        match self.auth_type {
            AuthType::Unsupported => {
                match self.unsupported.as_ref().and_then(unsupported_auth_name) {
                    Some(name) => SharedString::from(format!("{name} (kept as imported)")),
                    None => SharedString::from(AuthType::Unsupported.label()),
                }
            }
            auth_type => SharedString::from(auth_type.label()),
        }
    }

    /// The inherited source line (REQ-MODEL-01) under the type picker.
    fn render_inherited_source(&self, cx: &App) -> AnyElement {
        h_flex()
            .id(ElementId::Name(SharedString::new_static(
                "auth-inherited-source",
            )))
            .debug_selector(|| INHERITED_SOURCE_SELECTOR.to_string())
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(
                Label::new(self.inherited_source_label())
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .into_any_element()
    }

    /// The active type's form rows under the type row: a note in the control column
    /// for a type with nothing to edit, otherwise one row per field.
    fn render_type_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let note = |text: &'static str| {
            vec![form_row(
                None,
                muted_note(text).into_any_element(),
                None,
                cx,
            )]
        };
        match self.auth_type {
            AuthType::Inherit => note(
                "This request uses the auth its folder or collection sets — the source \
                 is named above.",
            ),
            AuthType::None => note(
                "This request is sent without credentials: no Authorization header is \
                 added to the send.",
            ),
            AuthType::Basic => vec![
                self.render_field("username", "Username", &self.basic_username_input, cx),
                self.render_field("password", "Password", &self.basic_password_input, cx),
            ],
            AuthType::Bearer => {
                vec![self.render_field("token", "Token", &self.bearer_token_input, cx)]
            }
            AuthType::ApiKey => vec![
                self.render_field("key", "Key", &self.api_key_name_input, cx),
                self.render_field("value", "Value", &self.api_key_value_input, cx),
                form_row(
                    Some(("location", "Add to")),
                    self.render_location_control(cx),
                    None,
                    cx,
                ),
            ],
            AuthType::Unsupported => note(
                "This auth uses a type Pustman does not edit yet (for example Digest or \
                 OAuth 2.0). It is kept exactly as imported until you choose a type above.",
            ),
        }
    }

    /// One labelled field: its name in the name column, its framed input in the
    /// control column.
    fn render_field(
        &self,
        key: &'static str,
        label: &'static str,
        input: &Entity<TextInput>,
        cx: &App,
    ) -> AnyElement {
        // A column, not a centred row: the input has no width of its own, so it takes
        // the frame's width only when the cross axis stretches it.
        let field = v_flex()
            .w_full()
            .h(px_to_rems(CONTROL_HEIGHT_PX))
            .justify_center()
            .min_w_0()
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_sm()
            .px(DynamicSpacing::Base06.rems(cx))
            .child(input.clone())
            .into_any_element();
        form_row(Some((key, label)), field, None, cx)
    }

    /// The REQ-VAR-SHARE-03 hint under the fields: a nudge, never a block.
    fn render_secret_hint(&self, cx: &App) -> impl IntoElement {
        h_flex()
            .id(ElementId::Name(SharedString::new_static(
                "auth-secret-hint",
            )))
            .debug_selector(|| SECRET_HINT_SELECTOR.to_string())
            .items_center()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(
                Icon::new(IconName::Braces)
                    .size(IconSize::Small)
                    .color(Color::Warning),
            )
            .child(
                Label::new(SharedString::new_static(
                    "This looks like a typed-in secret. Use a secret variable instead — \
                     a name like {{token}} whose value lives in the environment — so the \
                     value is never stored in the collection.",
                ))
                .size(LabelSize::Small)
                .color(Color::Warning),
            )
    }
}

impl Render for AuthTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let type_row = form_row(
            Some(("type", "Type")),
            self.render_type_control(cx),
            // Only Inherit takes its auth from a parent; for any other type the
            // source line would name a chain the send never reads.
            (self.auth_type == AuthType::Inherit).then(|| self.render_inherited_source(cx)),
            cx,
        );
        let hint = self.secret_hint.then(|| {
            form_row(
                None,
                self.render_secret_hint(cx).into_any_element(),
                None,
                cx,
            )
        });
        let rows = self.render_type_rows(cx);
        v_flex()
            .id(ElementId::Name(SharedString::new_static("auth-tab")))
            .size_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(rems(FORM_MAX_WIDTH_REMS))
                    .gap(DynamicSpacing::Base06.rems(cx))
                    .py(DynamicSpacing::Base06.rems(cx))
                    .child(type_row)
                    .children(rows)
                    .children(hint),
            )
    }
}

/// One form row: the name (if any) in the fixed name column, vertically centred on the
/// control, and the control — with an optional line under it — in the control column,
/// so every row of the tab lines up like the Settings tab.
fn form_row(
    name: Option<(&'static str, &'static str)>,
    control: AnyElement,
    under: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let label = h_flex()
        .flex_none()
        .w(px_to_rems(NAME_COLUMN_WIDTH_PX))
        .h(px_to_rems(CONTROL_HEIGHT_PX))
        .items_center()
        .children(name.map(|(key, text)| {
            div()
                .debug_selector(move || format!("auth-label-{key}"))
                .child(Label::new(SharedString::new_static(text)).color(Color::Muted))
        }));
    let control = match name {
        Some((key, _)) => div()
            .debug_selector(move || format!("auth-control-{key}"))
            .child(control)
            .into_any_element(),
        None => control,
    };
    h_flex()
        .items_start()
        .min_w_0()
        .gap(DynamicSpacing::Base08.rems(cx))
        .child(label)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(DynamicSpacing::Base02.rems(cx))
                .child(control)
                .children(under),
        )
        .into_any_element()
}

/// One stored, tab-reachable field: a single-line [`TextInput`] reporting edits back
/// to the tab. A credential field ([`AuthField::is_secret`]) runs `text_input`'s
/// masked mode, so its value never echoes in clear (#178) — and `is_secret` stays the
/// one list of which fields those are.
fn make_input(
    placeholder: &'static str,
    field: AuthField,
    window: &mut Window,
    cx: &mut Context<AuthTab>,
) -> Entity<TextInput> {
    let input = cx.new(|cx| {
        let input = TextInput::new(window, cx).with_placeholder(SharedString::from(placeholder));
        if field.is_secret() {
            input.with_masked()
        } else {
            input
        }
    });
    // `text_input`'s handle defaults to not being a tab stop; every field belongs in
    // the tab order (keyboard-first).
    input.read(cx).focus_handle(cx).tab_stop(true);
    input
}

/// Fill a stored field from the imported auth. The field's own echo of the change is
/// compared into a no-op by [`AuthTab::on_field_changed`].
fn fill(input: &Entity<TextInput>, text: Option<&str>, cx: &mut Context<AuthTab>) {
    input.update(cx, |input, cx| input.set_text(text.unwrap_or_default(), cx));
}

/// A muted paragraph — the note under a type that has nothing to edit.
fn muted_note(text: &'static str) -> impl IntoElement {
    Label::new(SharedString::new_static(text))
        .size(LabelSize::Small)
        .color(Color::Muted)
}

/// Where an API key goes on the wire (REQ-HTTP-06), as the picker names it.
fn location_label(location: ApiKeyLocation) -> &'static str {
    match location {
        ApiKeyLocation::Header => "Header",
        ApiKeyLocation::Query => "Query parameter",
    }
}

/// The `type` tag of an unmodelled auth ("digest", …) — what the type button names it.
fn unsupported_auth_name(auth: &Auth) -> Option<&str> {
    let Auth::Unsupported(value) = auth else {
        return None;
    };
    value.get("type").and_then(|tag| tag.as_str())
}

/// Whether `text` is a credential typed by hand rather than referenced: it carries
/// literal characters outside its `{{variable}}` placeholders (REQ-VAR-SHARE-03).
/// The placeholders come from the `variables` scanner — never a second brace parser.
fn has_literal_secret(text: &str) -> bool {
    let scopes = ScopeStack::new();
    let mut cursor = 0;
    for span in spans(text, &scopes) {
        if literal_between(text, cursor, span.range.start) {
            return true;
        }
        cursor = span.range.end;
    }
    literal_between(text, cursor, text.len())
}

/// Whether `text[start..end]` holds any character that is not whitespace. The
/// bounds come from [`spans`] over this very text, so they cannot split a character.
fn literal_between(text: &str, start: usize, end: usize) -> bool {
    text.get(start..end)
        .is_some_and(|gap| gap.chars().any(|character| !character.is_whitespace()))
}

#[cfg(test)]
#[path = "auth_tab_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "auth_tab_layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "auth_tab_menu_tooltip_tests.rs"]
mod menu_tooltip_tests;

#[cfg(test)]
#[path = "auth_tab_mask_tests.rs"]
mod mask_tests;
