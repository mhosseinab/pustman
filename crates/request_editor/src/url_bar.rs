//! The URL bar: the method picker and the URL field of a request editor (REQ-HTTP-01/02,
//! pack screen S03 "HTTP composer").
//!
//! `Decision:` the method dropdown composes the `ui` [`Button`] and [`ContextMenu`]
//! primitives — the step's constraint — instead of a popover-menu primitive `crates/ui`
//! does not have yet. While it is open it is drawn in a [`gpui::deferred`] floating
//! layer anchored under the trigger and snapped to the window edge, so it is never
//! clipped by the editor below it and never leaves the window.

use std::sync::Arc;

use api_model::Method;
use gpui::{
    Anchor, AnyElement, Entity, EventEmitter, Focusable, Subscription, Window, anchored, deferred,
};
use text_input::{TextInput, TextInputEvent};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;
use variables::ScopeStack;

use crate::method;
use crate::variable_autocomplete::VariableAutocomplete;
use crate::variable_highlight;
use crate::variable_hover::VariableHover;

/// The height of the URL row in pixels at the default scale — a [`ButtonSize::Large`],
/// the pack's `density.compact-row`. The method, the URL field and Send all stand on it
/// (#218), and the method dropdown is anchored just under it.
const URL_ROW_HEIGHT_PX: f32 = 32.;

/// The width of the custom method field in pixels at the default scale — room for the
/// longest plausible token (`PROPFIND`) without taking space from the URL.
const CUSTOM_FIELD_WIDTH_PX: f32 = 120.;

/// Test hook: the inline error that says why a custom method was rejected.
pub const CUSTOM_ERROR_SELECTOR: &str = "custom-method-error";

/// Test hook: the method control at the start of the URL row.
pub const METHOD_CONTROL_SELECTOR: &str = "method-control";

/// Test hook: the bordered URL field.
pub const URL_FIELD_SELECTOR: &str = "url-field";

/// What a [`UrlBar`] tells its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum UrlBarEvent {
    /// The URL text changed — typing, paste or [`UrlBar::set_url`].
    UrlChanged,
    /// A method was applied: a dropdown pick or an accepted custom token.
    MethodChanged(Method),
}

impl EventEmitter<UrlBarEvent> for UrlBar {}

/// One request's method picker and URL field.
///
/// The bar owns what it edits — the method and the URL text — and reports every applied
/// change as a [`UrlBarEvent`]; the owner keeps the request draft in step with it.
pub struct UrlBar {
    method: Method,
    url_input: Entity<TextInput>,
    /// The method button's focus handle. It is the flow's anchor: the dropdown and the
    /// custom method field hand focus back to it when they close.
    method_focus_handle: FocusHandle,
    /// The open dropdown and its subscription: dropped together when it dismisses.
    method_menu: Option<Entity<ContextMenu>>,
    method_menu_subscription: Option<Subscription>,
    /// The custom method field while a custom token is being typed.
    custom_input: Option<Entity<TextInput>>,
    custom_input_subscription: Option<Subscription>,
    /// Why the last typed custom method was not applied, shown under the field.
    custom_error: Option<SharedString>,
    /// The scopes the URL highlights its `{{variables}}` against (REQ-VAR-03) — the
    /// editor's stack, shared (an `Arc`) so edits can re-highlight from here.
    scopes: Arc<ScopeStack>,
    /// The `{{variable}}` autocomplete behind the URL field (REQ-VAR-04): the popover
    /// under the input, driven from the input's edits (`variable_autocomplete`).
    autocomplete: Entity<VariableAutocomplete>,
    /// The `{{variable}}` hover behind the URL field (REQ-VAR-03): the value and scope
    /// popover over a highlighted placeholder (`variable_hover`). It wraps the
    /// autocomplete, so the one attach point over the field hosts both affordances.
    hover: Entity<VariableHover>,
    _url_subscription: Subscription,
}

impl UrlBar {
    /// A bar for a request that already has a method and a URL.
    pub fn new(method: Method, url: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .with_placeholder(SharedString::new_static("Enter request URL"))
        });
        url_input.update(cx, |input, cx| input.set_text(&url, cx));
        // `text_input`'s handle defaults to not being a tab stop; the URL field belongs
        // in the tab order right after the method control.
        url_input.read(cx).focus_handle(cx).tab_stop(true);
        let url_subscription = cx.subscribe_in(&url_input, window, Self::on_url_event);
        // The `{{variable}}` popover completes this field (REQ-VAR-04), and the hover
        // over a highlighted placeholder wraps it (REQ-VAR-03).
        let autocomplete = cx.new(|cx| VariableAutocomplete::new(url_input.clone(), window, cx));
        let hover = cx.new(|cx| {
            VariableHover::new(url_input.clone(), autocomplete.clone().into(), window, cx)
        });
        let bar = Self {
            method,
            url_input,
            method_focus_handle: cx.focus_handle().tab_stop(true),
            method_menu: None,
            method_menu_subscription: None,
            custom_input: None,
            custom_input_subscription: None,
            custom_error: None,
            scopes: Arc::new(ScopeStack::new()),
            autocomplete,
            hover,
            _url_subscription: url_subscription,
        };
        bar.refresh_highlights(cx);
        bar
    }

    /// The URL field — what the variable highlighter and later steps reach into.
    pub fn url_input(&self) -> &Entity<TextInput> {
        &self.url_input
    }

    /// Store the scopes the URL highlights against and re-highlight it (REQ-VAR-03);
    /// the autocomplete offers their names (REQ-VAR-04) and the hover reads their
    /// values (REQ-VAR-03).
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.autocomplete.update(cx, |autocomplete, cx| {
            autocomplete.set_scopes(scopes.clone(), cx);
        });
        self.hover.update(cx, |hover, cx| {
            hover.set_scopes(scopes.clone(), cx);
        });
        self.scopes = scopes;
        self.refresh_highlights(cx);
    }

    /// The `{{variable}}` autocomplete behind the URL field (REQ-VAR-04) — what tests
    /// and later fields reach into.
    pub fn autocomplete(&self) -> &Entity<VariableAutocomplete> {
        &self.autocomplete
    }

    /// The `{{variable}}` hover behind the URL field (REQ-VAR-03) — what tests and the
    /// editor's secret wiring reach into.
    pub fn hover(&self) -> &Entity<VariableHover> {
        &self.hover
    }

    /// Re-highlight the URL against the current scopes.
    fn refresh_highlights(&self, cx: &mut Context<Self>) {
        variable_highlight::apply_to_input(&self.url_input, &self.scopes, cx);
    }

    /// The URL text.
    pub fn url(&self, cx: &App) -> String {
        self.url_input.read(cx).text()
    }

    /// Replace the URL text. The owner hears [`UrlBarEvent::UrlChanged`] like for any
    /// other edit, so the draft cannot drift from the field.
    pub fn set_url(&self, url: &str, cx: &mut App) {
        self.url_input
            .update(cx, |input, cx| input.set_text(url, cx));
    }

    /// The method the picker shows.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// The color the method control and its dropdown draw the method in: the theme's
    /// method token ([`ui::color::method_color`], the mapping History rows use), with
    /// the verb's text beside it (#217).
    pub fn method_label_color(&self, cx: &App) -> Color {
        method_color(self.method.as_str(), cx)
    }

    /// Why the last custom method was not applied, if one was rejected.
    pub fn custom_error(&self) -> Option<&SharedString> {
        self.custom_error.as_ref()
    }

    /// Move focus into the URL field (the editor's `request::FocusUrlBar` lands here).
    pub fn focus_url(&self, window: &mut Window, cx: &mut App) {
        let focus_handle = self.url_input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    /// Open the field a custom method token is typed into — what the dropdown's
    /// "Custom Method…" entry runs. Enter applies the token, Escape backs out.
    pub fn begin_custom_method(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.custom_input.is_some() {
            return;
        }
        self.custom_error = None;
        // A custom token already in place is what the user wants to edit; a standard
        // verb starts the field empty rather than inviting "GET" as a custom method.
        let initial = match &self.method {
            Method::Custom(token) => token.clone(),
            _ => String::new(),
        };
        let input = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(SharedString::new_static("e.g. PROPFIND"))
        });
        input.read(cx).focus_handle(cx).tab_stop(true);
        input.update(cx, |input, cx| input.set_text(&initial, cx));
        let subscription = cx.subscribe_in(&input, window, Self::on_custom_input_event);
        self.custom_input_subscription = Some(subscription);
        self.custom_input = Some(input.clone());
        let focus_handle = input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    /// Apply a method: the dropdown pick and an accepted custom token both land here.
    fn apply_method(&mut self, method: Method, window: &mut Window, cx: &mut Context<Self>) {
        self.method = method;
        self.custom_error = None;
        self.close_custom_input();
        cx.emit(UrlBarEvent::MethodChanged(self.method.clone()));
        window.focus(&self.method_focus_handle, cx);
        cx.notify();
    }

    /// Close the custom method field without applying anything.
    fn close_custom_input(&mut self) {
        self.custom_input = None;
        self.custom_input_subscription = None;
    }

    /// Open the method dropdown under the method button.
    fn open_method_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.method_menu.is_some() {
            return;
        }
        let picker = cx.weak_entity();
        let custom_picker = cx.weak_entity();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx);
            for standard in method::STANDARD_METHODS {
                let picked = standard.clone();
                let picker = picker.clone();
                // Each verb draws in its theme token, the same colour History rows
                // give it (#217).
                menu = menu.colored_entry(
                    standard.as_str(),
                    method_color(standard.as_str(), cx),
                    move |_event, window, cx| {
                        if picker
                            .update(cx, |bar, cx| bar.apply_method(picked.clone(), window, cx))
                            .is_err()
                        {
                            // The bar was dropped while the menu was open; there is no
                            // request left to apply a method to.
                        }
                    },
                );
            }
            menu.separator().entry(
                SharedString::new_static("Custom Method…"),
                move |_event, window, cx| {
                    if custom_picker
                        .update(cx, |bar, cx| bar.begin_custom_method(window, cx))
                        .is_err()
                    {
                        // The bar was dropped while the menu was open; there is no
                        // field left to type into.
                    }
                },
            )
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.method_menu = Some(menu);
        self.method_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The dropdown is done — an entry ran or Escape backed out — so it goes away and
    /// focus returns to the method button, unless the custom field just took it.
    fn on_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.method_menu = None;
        self.method_menu_subscription = None;
        if self.custom_input.is_none() {
            window.focus(&self.method_focus_handle, cx);
        }
        cx.notify();
    }

    /// The URL field changed; the owner tracks it through [`UrlBarEvent::UrlChanged`].
    fn on_url_event(
        &mut self,
        _input: &Entity<TextInput>,
        event: &TextInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event == TextInputEvent::Changed {
            self.refresh_highlights(cx);
            cx.emit(UrlBarEvent::UrlChanged);
        }
    }

    /// Enter in the custom method field applies the token it holds; a token the rule
    /// rejects is not applied and is explained inline instead.
    fn on_custom_input_event(
        &mut self,
        input: &Entity<TextInput>,
        event: &TextInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Submitted {
            return;
        }
        let typed = input.read(cx).text();
        match method::custom_method(&typed) {
            Ok(method) => self.apply_method(method, window, cx),
            Err(_) => {
                self.custom_error = Some(method::rejection_message(&typed));
                cx.notify();
            }
        }
    }

    /// `request::Cancel` (Escape): back out of the custom method field. With nothing to
    /// back out of here, the action keeps bubbling to the editor — S26b answers it with
    /// "cancel the in-flight send".
    fn cancel(
        &mut self,
        _: &pustman_actions::request::Cancel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.custom_input.is_none() {
            cx.propagate();
            return;
        }
        self.close_custom_input();
        self.custom_error = None;
        window.focus(&self.method_focus_handle, cx);
        cx.notify();
    }

    /// The method control: the dropdown trigger, or the custom method field while a
    /// custom token is being typed. The dropdown floats under it.
    fn render_method_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let control = match self.custom_input.clone() {
            Some(input) => div()
                .flex()
                .flex_col()
                .justify_center()
                .w(px_to_rems(CUSTOM_FIELD_WIDTH_PX))
                .h_full()
                .px(DynamicSpacing::Base04.rems(cx))
                .child(input)
                .into_any_element(),
            None => Button::new(
                ElementId::from(&self.method_focus_handle),
                SharedString::from(self.method.as_str()),
            )
            // Medium inside the 32 px frame: the frame's border is the row's edge.
            .size(ButtonSize::Medium)
            // The verb draws in its theme token, text and colour together (#217).
            .label_color(self.method_label_color(cx))
            .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
            // The tooltip hides while its own dropdown is open (#265): with the pointer
            // still on the anchor, the balloon would paint over the menu rows.
            .tooltip(Tooltip::text("Choose the HTTP method").hidden(self.method_menu.is_some()))
            .focus_handle(self.method_focus_handle.clone())
            .on_click(cx.listener(|this, _event, window, cx| this.open_method_menu(window, cx)))
            .into_any_element(),
        };
        // The method half of the one bordered URL control: framed on three sides, the
        // URL field's own left border is the separator between them.
        div()
            .debug_selector(|| METHOD_CONTROL_SELECTOR.to_string())
            .relative()
            .flex()
            .items_center()
            .h(px_to_rems(URL_ROW_HEIGHT_PX))
            .px(DynamicSpacing::Base01.rems(cx))
            .border_l_1()
            .border_t_1()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .rounded_l_sm()
            .bg(cx.theme().colors().editor_background)
            .child(control)
            .children(self.method_menu.clone().map(|menu| {
                div()
                    .absolute()
                    .top(px_to_rems(URL_ROW_HEIGHT_PX))
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
            }))
            .into_any_element()
    }

    /// The inline error under the bar: what the rejected token was and why.
    fn render_custom_error(&self, message: SharedString) -> impl IntoElement {
        h_flex()
            .id(ElementId::NamedChild(
                Arc::new(ElementId::from(&self.method_focus_handle)),
                SharedString::new_static("custom-method-error"),
            ))
            .debug_selector(|| CUSTOM_ERROR_SELECTOR.to_string())
            .child(
                Label::new(message)
                    .size(LabelSize::Small)
                    .color(Color::Error),
            )
    }
}

impl Render for UrlBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(ElementId::from(&self.method_focus_handle))
            .gap(DynamicSpacing::Base02.rems(cx))
            .on_action(cx.listener(Self::cancel))
            .child(
                // One bordered control: method, separator, URL (mockup section 1).
                h_flex()
                    .child(self.render_method_control(cx))
                    .child(
                        div()
                            .debug_selector(|| URL_FIELD_SELECTOR.to_string())
                            .flex()
                            .flex_col()
                            .justify_center()
                            .flex_1()
                            .min_w_0()
                            .h(px_to_rems(URL_ROW_HEIGHT_PX))
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .rounded_r_sm()
                            .bg(cx.theme().colors().editor_background)
                            .px(DynamicSpacing::Base04.rems(cx))
                            .child(self.hover.clone()),
                    ), // S26b's Send button lands at the end of this row.
            )
            .children(
                self.custom_error
                    .clone()
                    .map(|message| self.render_custom_error(message)),
            )
    }
}

#[cfg(test)]
#[path = "url_bar_field_width_tests.rs"]
mod field_width_tests;

#[cfg(test)]
#[path = "url_bar_menu_tooltip_tests.rs"]
mod menu_tooltip_tests;

#[cfg(test)]
#[path = "url_bar_method_color_tests.rs"]
mod method_color_tests;
