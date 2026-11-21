//! The Settings tab (PRD REQ-HTTP-09, REQ-HTTP-10): every [`RequestSettings`] override
//! this request carries — timeout, redirects, TLS verification, URL encoding, Referer
//! and HTTP version — each with the "use global default" state [`None`] means (the
//! model: every field is an [`Option`], `None` inherits the global default).
//!
//! `Decision:` the value shown next to the "use global default" state comes from
//! [`SendSettings::default`] — the base the send pipeline merges the request's overrides
//! over (`http_engine`'s `pipeline::send_settings`), so what the label promises is what
//! the send actually inherits. The `settings` crate's `request.*` global was checked
//! first: it covers four of the eight fields and is not the pipeline's base yet, so
//! showing its values would promise sends that do not happen.
//!
//! `Decision:` `RequestSettings::remove_referer` has no [`SendSettings`] field — the
//! pipeline drops it — so there is no code default to show and none is invented here:
//! its "use global default" state reads without a value, and turning the checkbox off
//! pins `false`, the switch's position. The override is still edited and stored for the
//! workspace format (REQ-MODEL-03); a follow-up that honours it on the wire owes this
//! row its default.
//!
//! `Decision:` each control reports the one [`SettingChange`] it made rather than the
//! tab's whole [`RequestSettings`], so the owner writes exactly the field the user
//! edited. A cURL import replaces the whole request without re-seeding this tab
//! (`curl_paste` is outside this step's Edits), and per-field writes keep such a stale
//! tab from dropping the imported overrides — notably `-k`/`--insecure`'s `verify_tls`.
//! The tab re-seeds from the draft when it is selected (`RequestEditor`), so the stale
//! display heals the moment the user comes back to it.
//!
//! `Decision:` a number that does not parse leaves the draft's override alone and says
//! so under its field: a typo must not silently fall back to the global default.

use api_model::{HttpRequest, HttpVersion, RequestSettings};
use gpui::{
    Anchor, AnyElement, App, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, ParentElement, Render, SharedString, Subscription, WeakEntity, Window, anchored,
    deferred,
};
use http_engine::SendSettings;
use text_input::{TextInput, TextInputEvent};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;

/// The height of one control (a [`ButtonSize::Medium`]) in pixels at the default
/// scale: the version picker menu is anchored just under it (`auth_tab`).
const CONTROL_HEIGHT_PX: f32 = 28.;

/// The width of the numeric fields in pixels at the default scale: room for about six
/// digits and the unit beside them. An unfixed field in a flex row renders at its caret
/// width — a few pixels (S33n).
const NUMBER_INPUT_WIDTH_PX: f32 = 96.;

/// The control column's width in pixels at the default scale: the widest control (the
/// numeric field) fits, so every switch, field and picker starts on one vertical line.
const CONTROL_COLUMN_WIDTH_PX: f32 = 128.;

/// The "use global default" column's width in pixels at the default scale: room for
/// "Use global default (30000 ms)" at the small label size.
const DEFAULT_COLUMN_WIDTH_PX: f32 = 232.;

/// The form's widest extent in rems: past it the name column only adds empty space
/// between a setting's name and its control.
const FORM_MAX_WIDTH_REMS: f32 = 52.;

/// Test hook: the HTTP version picker.
const HTTP_VERSION_CONTROL_SELECTOR: &str = "settings-http-version-control";

/// Test hook: the verify-TLS switch of the Settings tab.
pub const VERIFY_TLS_SWITCH_SELECTOR: &str = "settings-verify-tls-switch";

/// Test hook: its "use global default" checkbox.
pub const VERIFY_TLS_DEFAULT_SELECTOR: &str = "settings-verify-tls-default";

/// One per-request override the Settings tab edits, with its new state: [`None`] is the
/// "use global default" state, `Some(value)` pins the value for this request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingChange {
    TimeoutMs(Option<u64>),
    FollowRedirects(Option<bool>),
    MaxRedirects(Option<u32>),
    FollowOriginalMethod(Option<bool>),
    VerifyTls(Option<bool>),
    EncodeUrl(Option<bool>),
    RemoveReferer(Option<bool>),
    HttpVersion(Option<HttpVersion>),
}

impl SettingChange {
    /// Write the change into `settings` — the one mapping from a field to the model.
    pub fn apply(self, settings: &mut RequestSettings) {
        match self {
            SettingChange::TimeoutMs(value) => settings.timeout_ms = value,
            SettingChange::FollowRedirects(value) => settings.follow_redirects = value,
            SettingChange::MaxRedirects(value) => settings.max_redirects = value,
            SettingChange::FollowOriginalMethod(value) => {
                settings.follow_original_method = value;
            }
            SettingChange::VerifyTls(value) => settings.verify_tls = value,
            SettingChange::EncodeUrl(value) => settings.encode_url = value,
            SettingChange::RemoveReferer(value) => settings.remove_referer = value,
            SettingChange::HttpVersion(value) => settings.http_version = value,
        }
    }
}

/// What a [`SettingsTab`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTabEvent {
    /// One override changed: [`SettingChange::apply`] is the change for the draft's
    /// settings.
    SettingChanged(SettingChange),
}

impl EventEmitter<SettingsTabEvent> for SettingsTab {}

/// The two numeric overrides: the one place their two types differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumericField {
    TimeoutMs,
    MaxRedirects,
}

impl NumericField {
    /// The key every test hook of this row is derived from.
    fn key(self) -> &'static str {
        match self {
            NumericField::TimeoutMs => "settings-timeout",
            NumericField::MaxRedirects => "settings-max-redirects",
        }
    }

    /// The value as the field shows it: the override, or the global default it
    /// inherits.
    fn display(self, tab: &SettingsTab) -> String {
        match self {
            NumericField::TimeoutMs => tab
                .timeout_ms
                .unwrap_or_else(default_timeout_ms)
                .to_string(),
            NumericField::MaxRedirects => tab
                .max_redirects
                .unwrap_or_else(default_max_redirects)
                .to_string(),
        }
    }

    /// The default's value as the "use global default" state names it.
    fn default_display(self) -> String {
        match self {
            NumericField::TimeoutMs => format!("{} ms", default_timeout_ms()),
            NumericField::MaxRedirects => default_max_redirects().to_string(),
        }
    }

    /// The change an edit of `text` makes, or [`None`] when it is not a number.
    fn parse_change(self, text: &str) -> Option<SettingChange> {
        match self {
            NumericField::TimeoutMs => text
                .parse::<u64>()
                .ok()
                .map(|value| SettingChange::TimeoutMs(Some(value))),
            NumericField::MaxRedirects => text
                .parse::<u32>()
                .ok()
                .map(|value| SettingChange::MaxRedirects(Some(value))),
        }
    }

    /// The change the "use global default" checkbox makes.
    fn use_default_change(self, use_default: bool) -> SettingChange {
        match self {
            NumericField::TimeoutMs => {
                SettingChange::TimeoutMs((!use_default).then(default_timeout_ms))
            }
            NumericField::MaxRedirects => {
                SettingChange::MaxRedirects((!use_default).then(default_max_redirects))
            }
        }
    }

    /// What to say when the text is not a number.
    fn invalid_note(self) -> &'static str {
        match self {
            NumericField::TimeoutMs => "Enter a whole number of milliseconds.",
            NumericField::MaxRedirects => "Enter a whole number of redirects.",
        }
    }

    /// Whether this field is in the "use global default" state.
    fn is_inherit(self, tab: &SettingsTab) -> bool {
        match self {
            NumericField::TimeoutMs => tab.timeout_ms.is_none(),
            NumericField::MaxRedirects => tab.max_redirects.is_none(),
        }
    }

    /// Whether the last edit of this field did not parse.
    fn is_invalid(self, tab: &SettingsTab) -> bool {
        match self {
            NumericField::TimeoutMs => tab.timeout_invalid,
            NumericField::MaxRedirects => tab.max_redirects_invalid,
        }
    }

    /// Remember whether the last edit of this field parsed.
    fn set_invalid(self, tab: &mut SettingsTab, invalid: bool) {
        match self {
            NumericField::TimeoutMs => tab.timeout_invalid = invalid,
            NumericField::MaxRedirects => tab.max_redirects_invalid = invalid,
        }
    }
}

/// One boolean row as the tab draws it: its meaning, its three states, and the test
/// hooks of its two controls.
struct BoolRow {
    name: &'static str,
    description: &'static str,
    change: fn(Option<bool>) -> SettingChange,
    value: Option<bool>,
    default: Option<bool>,
    switch_selector: &'static str,
    default_selector: &'static str,
}

/// The Settings tab: one row per [`RequestSettings`] field, each showing whether this
/// request inherits the global default or pins its own value.
pub struct SettingsTab {
    timeout_ms: Option<u64>,
    follow_redirects: Option<bool>,
    max_redirects: Option<u32>,
    follow_original_method: Option<bool>,
    verify_tls: Option<bool>,
    encode_url: Option<bool>,
    remove_referer: Option<bool>,
    http_version: Option<HttpVersion>,
    // The numeric fields' mirrors: created once and kept, so editing one keeps its
    // focus and caret (the `auth_tab` fields' rule). Canonical state is the values
    // above; a programmatic fill echoes [`TextInputEvent::Changed`] and is compared
    // against them into a no-op.
    timeout_input: Entity<TextInput>,
    max_redirects_input: Entity<TextInput>,
    timeout_invalid: bool,
    max_redirects_invalid: bool,
    /// The open version picker menu and its subscription, dropped together when it
    /// dismisses.
    version_menu: Option<Entity<ContextMenu>>,
    version_menu_subscription: Option<Subscription>,
    /// The version button's focus handle: the menu hands focus back to it when it closes.
    version_focus_handle: FocusHandle,
    _input_subscriptions: Vec<Subscription>,
}

impl SettingsTab {
    /// The tab for `request`: every override seeded from its settings, so the tab
    /// round-trips what the request already carries until the user edits it.
    pub fn new(request: &HttpRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let timeout_input = make_input(window, cx);
        let max_redirects_input = make_input(window, cx);
        let mut tab = Self {
            timeout_ms: None,
            follow_redirects: None,
            max_redirects: None,
            follow_original_method: None,
            verify_tls: None,
            encode_url: None,
            remove_referer: None,
            http_version: None,
            timeout_input,
            max_redirects_input,
            timeout_invalid: false,
            max_redirects_invalid: false,
            version_menu: None,
            version_menu_subscription: None,
            version_focus_handle: cx.focus_handle().tab_stop(true),
            _input_subscriptions: Vec::new(),
        };
        // Seed before the fields are wired back to the tab, so the fills are not
        // edits: they never reach the request draft.
        tab.reseed(request, cx);
        tab._input_subscriptions = [
            (NumericField::TimeoutMs, &tab.timeout_input),
            (NumericField::MaxRedirects, &tab.max_redirects_input),
        ]
        .into_iter()
        .map(|(field, input)| {
            cx.subscribe_in(input, window, move |tab, _input, event, _window, cx| {
                tab.on_numeric_changed(field, event, cx);
            })
        })
        .collect();
        tab
    }

    /// Re-read every override from `request` — the open path, and the re-seed after a
    /// whole-request replacement. Not an edit: nothing is emitted toward the draft.
    pub fn reseed(&mut self, request: &HttpRequest, cx: &mut Context<Self>) {
        let settings = request.settings.clone().unwrap_or_default();
        self.timeout_ms = settings.timeout_ms;
        self.follow_redirects = settings.follow_redirects;
        self.max_redirects = settings.max_redirects;
        self.follow_original_method = settings.follow_original_method;
        self.verify_tls = settings.verify_tls;
        self.encode_url = settings.encode_url;
        self.remove_referer = settings.remove_referer;
        self.http_version = settings.http_version;
        self.fill_inputs(cx);
        cx.notify();
    }

    /// The verify-TLS override this tab holds — what the TLS failure's offer writes
    /// and tests read back.
    pub fn verify_tls(&self) -> Option<bool> {
        self.verify_tls
    }

    /// Record one override change and hand it to the owner. A change equal to the
    /// state already held is a no-op: a switch flipped to the state it already shows,
    /// or the echo of a programmatic fill, is not an edit.
    fn apply(&mut self, change: SettingChange, cx: &mut Context<Self>) {
        let changed = match change {
            SettingChange::TimeoutMs(value) => set_if_changed(&mut self.timeout_ms, value),
            SettingChange::FollowRedirects(value) => {
                set_if_changed(&mut self.follow_redirects, value)
            }
            SettingChange::MaxRedirects(value) => set_if_changed(&mut self.max_redirects, value),
            SettingChange::FollowOriginalMethod(value) => {
                set_if_changed(&mut self.follow_original_method, value)
            }
            SettingChange::VerifyTls(value) => set_if_changed(&mut self.verify_tls, value),
            SettingChange::EncodeUrl(value) => set_if_changed(&mut self.encode_url, value),
            SettingChange::RemoveReferer(value) => set_if_changed(&mut self.remove_referer, value),
            SettingChange::HttpVersion(value) => set_if_changed(&mut self.http_version, value),
        };
        if !changed {
            return;
        }
        cx.emit(SettingsTabEvent::SettingChanged(change));
        cx.notify();
    }

    /// The stored input behind `field`.
    fn numeric_input(&self, field: NumericField) -> &Entity<TextInput> {
        match field {
            NumericField::TimeoutMs => &self.timeout_input,
            NumericField::MaxRedirects => &self.max_redirects_input,
        }
    }

    /// A numeric field was edited (or filled programmatically — the fill's echo shows
    /// the text the tab already holds and stops here, the `auth_tab` rule).
    fn on_numeric_changed(
        &mut self,
        field: NumericField,
        event: &TextInputEvent,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Changed {
            return;
        }
        let text = self.numeric_input(field).read(cx).text();
        if text == field.display(self) {
            return;
        }
        match field.parse_change(&text) {
            Some(change) => {
                field.set_invalid(self, false);
                self.apply(change, cx);
            }
            None => {
                field.set_invalid(self, true);
                cx.notify();
            }
        }
    }

    /// Re-fill the numeric fields from the tab's state — after a re-seed or a return
    /// to the "use global default" state, where the shown value changes under the
    /// user.
    fn fill_inputs(&mut self, cx: &mut Context<Self>) {
        let timeout = NumericField::TimeoutMs.display(self);
        let max_redirects = NumericField::MaxRedirects.display(self);
        fill(&self.timeout_input, &timeout, cx);
        fill(&self.max_redirects_input, &max_redirects, cx);
        self.timeout_invalid = false;
        self.max_redirects_invalid = false;
    }

    /// Open the HTTP version menu under its button.
    fn open_version_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.version_menu.is_some() {
            return;
        }
        let tab = cx.weak_entity();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx);
            for version in [HttpVersion::Auto, HttpVersion::Http11, HttpVersion::Http2] {
                let tab = tab.clone();
                menu = menu.entry(version_label(version), move |_event, _window, cx| {
                    if tab
                        .update(cx, |tab, cx| {
                            tab.apply(SettingChange::HttpVersion(Some(version)), cx);
                        })
                        .is_err()
                    {
                        // Handled: the tab dropped while the menu was open; no version
                        // left to choose.
                    }
                });
            }
            menu
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_version_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.version_menu = Some(menu);
        self.version_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The version menu is done — an entry ran or Escape backed out.
    fn on_version_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.version_menu = None;
        self.version_menu_subscription = None;
        window.focus(&self.version_focus_handle, cx);
        cx.notify();
    }

    /// One boolean override: its switch (the value the send uses) in the control column,
    /// the "use global default" checkbox (the state) in the default column, the
    /// default's value named next to that state. `default` is [`None`] where no code
    /// default exists (`remove_referer`).
    fn render_bool_row(
        &self,
        row: BoolRow,
        weak: &WeakEntity<SettingsTab>,
        cx: &App,
    ) -> AnyElement {
        let BoolRow {
            name,
            description,
            change,
            value,
            default,
            switch_selector,
            default_selector,
        } = row;
        let switch_weak = weak.clone();
        let checkbox_weak = weak.clone();
        let checkbox_label = use_default_label(
            value.is_none(),
            default.map(|default| on_off(default).to_string()),
        );
        let pinned = value.or(default).unwrap_or(false);
        let control = h_flex()
            .id(ElementId::Name(SharedString::from(format!(
                "{switch_selector}-wrap"
            ))))
            .debug_selector(|| switch_selector.to_string())
            .child(
                Toggle::new(
                    ElementId::Name(SharedString::from(switch_selector)),
                    SharedString::from(name),
                    ToggleState::from(pinned),
                )
                .kind(ToggleKind::Switch)
                // The row's name column names it on screen; the label stays its
                // accessible name.
                .label_hidden()
                .on_change(move |state, _window, cx| {
                    let change = change(Some(state.is_selected()));
                    if switch_weak
                        .update(cx, |tab, cx| tab.apply(change, cx))
                        .is_err()
                    {
                        // Handled: the tab closed with its window; no setting left to
                        // change.
                    }
                }),
            );
        let use_default = h_flex()
            .id(ElementId::Name(SharedString::from(format!(
                "{default_selector}-wrap"
            ))))
            .debug_selector(|| default_selector.to_string())
            .child(
                Toggle::new(
                    ElementId::Name(SharedString::from(default_selector)),
                    SharedString::from(checkbox_label),
                    ToggleState::from(value.is_none()),
                )
                .on_change(move |state, _window, cx| {
                    let use_default = state.is_selected();
                    let change = change((!use_default).then(|| value.or(default).unwrap_or(false)));
                    if checkbox_weak
                        .update(cx, |tab, cx| tab.apply(change, cx))
                        .is_err()
                    {
                        // Handled: the tab closed with its window; no setting left to
                        // change.
                    }
                }),
            );
        settings_row(
            SharedString::from(format!("settings-row-{switch_selector}")),
            name,
            description,
            control.into_any_element(),
            use_default.into_any_element(),
            None,
            cx,
        )
    }

    /// One numeric override: its value field (the value the send uses) in the control
    /// column, the "use global default" checkbox in the default column, and the parse
    /// note under the row while the text is not a number.
    fn render_numeric_row(
        &self,
        field: NumericField,
        name: &'static str,
        description: &'static str,
        weak: &WeakEntity<SettingsTab>,
        cx: &App,
    ) -> AnyElement {
        let key = field.key();
        let checkbox_key = format!("{key}-default");
        let checkbox_weak = weak.clone();
        let checkbox_label =
            use_default_label(field.is_inherit(self), Some(field.default_display()));
        let control = div()
            .id(ElementId::Name(SharedString::from(format!("{key}-input"))))
            .debug_selector(|| format!("{key}-input"))
            .w(px_to_rems(NUMBER_INPUT_WIDTH_PX))
            .child(self.numeric_input(field).clone());
        let use_default = h_flex()
            .id(ElementId::Name(SharedString::from(checkbox_key.clone())))
            .debug_selector(move || checkbox_key.clone())
            .child(
                Toggle::new(
                    ElementId::Name(SharedString::from(format!("{key}-default-toggle"))),
                    SharedString::from(checkbox_label),
                    ToggleState::from(field.is_inherit(self)),
                )
                .on_change(move |state, _window, cx| {
                    let change = field.use_default_change(state.is_selected());
                    if checkbox_weak
                        .update(cx, |tab, cx| {
                            tab.apply(change, cx);
                            tab.fill_inputs(cx);
                        })
                        .is_err()
                    {
                        // Handled: the tab closed with its window; no setting left to
                        // change.
                    }
                }),
            );
        let note = field.is_invalid(self).then(|| {
            Label::new(SharedString::from(field.invalid_note()))
                .size(LabelSize::XSmall)
                .color(Color::Error)
                .into_any_element()
        });
        settings_row(
            SharedString::from(format!("settings-row-{key}")),
            name,
            description,
            control.into_any_element(),
            use_default.into_any_element(),
            note,
            cx,
        )
    }

    /// The HTTP version override: the picker (the value the send uses) in the control
    /// column, the "use global default" checkbox in the default column.
    fn render_version_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let default = SendSettings::default().http_version;
        let checkbox_key = "settings-http-version-default";
        let checkbox_label = use_default_label(
            self.http_version.is_none(),
            Some(version_label(default).to_string()),
        );
        let use_default = h_flex()
            .id(ElementId::Name(SharedString::from(
                checkbox_key.to_string(),
            )))
            .debug_selector(|| checkbox_key.to_string())
            .child(
                Toggle::new(
                    ElementId::Name(SharedString::new_static(
                        "settings-http-version-default-toggle",
                    )),
                    SharedString::from(checkbox_label),
                    ToggleState::from(self.http_version.is_none()),
                )
                .on_change(move |state, _window, cx| {
                    let change =
                        SettingChange::HttpVersion((!state.is_selected()).then_some(default));
                    if weak.update(cx, |tab, cx| tab.apply(change, cx)).is_err() {
                        // Handled: the tab closed with its window; no version left to
                        // change.
                    }
                }),
            );
        let control = self.render_version_control(cx);
        settings_row(
            SharedString::new_static("settings-row-http-version"),
            "HTTP version",
            "The protocol version this request is sent with.",
            control,
            use_default.into_any_element(),
            None,
            cx,
        )
    }

    /// The version control: the dropdown trigger. The menu floats under it.
    fn render_version_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self
            .http_version
            .unwrap_or_else(|| SendSettings::default().http_version);
        h_flex()
            .relative()
            .debug_selector(|| HTTP_VERSION_CONTROL_SELECTOR.to_owned())
            .child(
                Button::new(
                    ElementId::from(&self.version_focus_handle),
                    SharedString::from(version_label(chosen)),
                )
                .size(ButtonSize::Medium)
                .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
                .tooltip(Tooltip::text(
                    "Choose the HTTP version this request sends with",
                ))
                .focus_handle(self.version_focus_handle.clone())
                .on_click(
                    cx.listener(|this, _event, window, cx| this.open_version_menu(window, cx)),
                ),
            )
            .children(floating_menu(&self.version_menu, cx))
            .into_any_element()
    }
}

impl Render for SettingsTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        let defaults = SendSettings::default();
        let connection = [
            self.render_numeric_row(
                NumericField::TimeoutMs,
                "Timeout (ms)",
                "How long to wait for a response before giving up.",
                &weak,
                cx,
            ),
            self.render_version_row(cx),
            self.render_bool_row(
                BoolRow {
                    name: "Verify TLS certificates",
                    description: "Reject servers whose certificate is not trusted.",
                    change: SettingChange::VerifyTls,
                    value: self.verify_tls,
                    default: Some(defaults.verify_tls),
                    switch_selector: VERIFY_TLS_SWITCH_SELECTOR,
                    default_selector: VERIFY_TLS_DEFAULT_SELECTOR,
                },
                &weak,
                cx,
            ),
        ];
        let redirects = [
            self.render_bool_row(
                BoolRow {
                    name: "Follow redirects",
                    description: "Follow 3xx responses to their Location.",
                    change: SettingChange::FollowRedirects,
                    value: self.follow_redirects,
                    default: Some(defaults.follow_redirects),
                    switch_selector: "settings-follow-redirects-switch",
                    default_selector: "settings-follow-redirects-default",
                },
                &weak,
                cx,
            ),
            self.render_numeric_row(
                NumericField::MaxRedirects,
                "Max redirects",
                "Stop after this many redirects.",
                &weak,
                cx,
            ),
            self.render_bool_row(
                BoolRow {
                    name: "Follow original method",
                    description: "Keep the method and body on 301/302 instead of switching to GET.",
                    change: SettingChange::FollowOriginalMethod,
                    value: self.follow_original_method,
                    default: Some(defaults.follow_original_method),
                    switch_selector: "settings-follow-original-method-switch",
                    default_selector: "settings-follow-original-method-default",
                },
                &weak,
                cx,
            ),
        ];
        let url_and_headers = [
            self.render_bool_row(
                BoolRow {
                    name: "Encode URL automatically",
                    description: "Percent-encode reserved characters in the URL path and query.",
                    change: SettingChange::EncodeUrl,
                    value: self.encode_url,
                    default: Some(defaults.encode_url),
                    switch_selector: "settings-encode-url-switch",
                    default_selector: "settings-encode-url-default",
                },
                &weak,
                cx,
            ),
            self.render_bool_row(
                BoolRow {
                    name: "Remove Referer header",
                    description: "Drop the Referer header when following a redirect.",
                    change: SettingChange::RemoveReferer,
                    value: self.remove_referer,
                    default: None,
                    switch_selector: "settings-remove-referer-switch",
                    default_selector: "settings-remove-referer-default",
                },
                &weak,
                cx,
            ),
        ];
        v_flex()
            .id(ElementId::Name(SharedString::new_static("settings-tab")))
            .size_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(rems(FORM_MAX_WIDTH_REMS))
                    .gap(DynamicSpacing::Base16.rems(cx))
                    .py(DynamicSpacing::Base06.rems(cx))
                    .child(settings_section("Connection", connection, cx))
                    .child(settings_section("Redirects", redirects, cx))
                    .child(settings_section("URL & headers", url_and_headers, cx)),
            )
    }
}

/// One titled group of rows, separated by hairlines.
fn settings_section(
    title: &'static str,
    rows: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let border = cx.theme().colors().border_variant;
    v_flex()
        .min_w_0()
        .child(
            div().pb(DynamicSpacing::Base04.rems(cx)).child(
                Label::new(SharedString::new_static(title))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            ),
        )
        .child(
            v_flex()
                .min_w_0()
                .border_t_1()
                .border_color(border)
                .children(
                    rows.into_iter()
                        .map(|row| div().border_b_1().border_color(border).child(row)),
                ),
        )
}

/// One setting: its name and what it does on the left, then the control and the
/// "use global default" checkbox in fixed columns, so every row's controls line up.
fn settings_row(
    id: SharedString,
    name: &'static str,
    description: &'static str,
    control: AnyElement,
    use_default: AnyElement,
    note: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    h_flex()
        .id(ElementId::Name(id))
        .items_center()
        .gap(DynamicSpacing::Base12.rems(cx))
        .py(DynamicSpacing::Base06.rems(cx))
        .min_w_0()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(DynamicSpacing::Base01.rems(cx))
                .child(Label::new(SharedString::new_static(name)).truncate())
                .child(
                    Label::new(SharedString::new_static(description))
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                ),
        )
        .child(
            v_flex()
                .flex_none()
                .w(px_to_rems(CONTROL_COLUMN_WIDTH_PX))
                .items_start()
                .gap(DynamicSpacing::Base01.rems(cx))
                .child(control)
                .children(note),
        )
        .child(
            div()
                .flex_none()
                .w(px_to_rems(DEFAULT_COLUMN_WIDTH_PX))
                .flex()
                .items_center()
                .child(use_default),
        )
        .into_any_element()
}

/// Store `value` in `slot` — whether it changed. Equal values are not edits.
fn set_if_changed<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

/// The default timeout in milliseconds — [`SendSettings::default`]'s own value, never
/// a duplicated literal.
fn default_timeout_ms() -> u64 {
    SendSettings::default().timeout.as_millis() as u64
}

/// The default redirect limit — [`SendSettings::default`]'s own value.
fn default_max_redirects() -> u32 {
    SendSettings::default().max_redirects
}

/// How the rows spell an on/off default.
fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

/// The "use global default" checkbox's label: the default's value next to the
/// "default" state, and the bare phrase while the request pins its own value.
fn use_default_label(in_default_state: bool, default_display: Option<String>) -> String {
    match default_display {
        Some(display) if in_default_state => format!("Use global default ({display})"),
        _ => String::from("Use global default"),
    }
}

/// How the version picker names each [`HttpVersion`].
fn version_label(version: HttpVersion) -> &'static str {
    match version {
        HttpVersion::Auto => "Auto",
        HttpVersion::Http11 => "HTTP/1.1",
        HttpVersion::Http2 => "HTTP/2",
    }
}

/// One stored, tab-reachable numeric field: a single-line [`TextInput`] reporting
/// edits back to the tab (`auth_tab`'s `make_input`, without a placeholder: the field
/// always shows the value the send uses).
fn make_input(window: &mut Window, cx: &mut Context<SettingsTab>) -> Entity<TextInput> {
    let input = cx.new(|cx| TextInput::new(window, cx));
    // `text_input`'s handle defaults to not being a tab stop; every field belongs in
    // the tab order (keyboard-first).
    input.read(cx).focus_handle(cx).tab_stop(true);
    input
}

/// Fill a stored field from the tab's state. The field's own echo of the change is
/// compared into a no-op by [`SettingsTab::on_numeric_changed`].
fn fill(input: &Entity<TextInput>, text: &str, cx: &mut Context<SettingsTab>) {
    input.update(cx, |input, cx| input.set_text(text, cx));
}

/// A picker menu floating under its trigger — the `auth_tab` shape (see that module's
/// note on hoisting this helper into `ui`): anchored so it is never clipped by the
/// rows below it, snapped to the window edge so it never leaves the window.
fn floating_menu(menu: &Option<Entity<ContextMenu>>, cx: &App) -> Option<AnyElement> {
    menu.clone().map(|menu| {
        div()
            .absolute()
            .top(px_to_rems(CONTROL_HEIGHT_PX))
            .left_0()
            .child(
                deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                        .child(menu),
                )
                .with_priority(1),
            )
            .into_any_element()
    })
}

#[cfg(test)]
#[path = "settings_tab_tests.rs"]
mod tests;
