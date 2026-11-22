//! `{{variable}}` hover behind the request fields (PRD REQ-VAR-03): hovering a
//! highlighted placeholder opens a popover with the name, the scope kind it resolves
//! from and its value — a secret is masked (never the value, never its length) with a
//! reveal toggle whose keychain read runs on a background task (the
//! `environment_picker` quick-look's reveal shape: masked → checking →
//! revealed/unset/error).
//!
//! `Decision:` the surface is `variable_autocomplete`'s anchored [`deferred`] popover
//! shape, not a [`ui::Tooltip`](ui::components::tooltip): the reveal toggle must be a
//! focusable control and a GPUI tooltip is a passive overlay. [`UrlBar`](crate::url_bar)
//! keeps the one attach point over the field: this hover wraps the autocomplete.
//!
//! `Decision:` the reveal reads the keychain through [`SecretSources`] —
//! `http_engine::prepare`'s shared boundary: the store plus the one closure from a
//! variable's scope kind and name to its [`SecretKey`], because only the caller knows
//! the workspace and scope ids. The later wiring step provides them through the same
//! setter; until then nothing can reach the OS keychain (a keychain read can prompt,
//! macOS) and a secret hovers as "unset".
//!
//! `Decision:` a secret's hover value comes only from that lookup, never from
//! [`ScopeStack`]'s local values (REQ-VAR-SHARE-02: a secret's plaintext lives in the
//! keychain). A revealed value lands in no tooltip, element id or debug string: the
//! cell truncates without a tooltip, the one truncation here that carries no full
//! value (AGENTS.md › Security, the quick-look's rule). Non-secret values truncate with
//! their full value in the tooltip as the design system requires.
//!
//! `Decision:` only the URL bar carries the hover in this step (`url_bar` attaches it);
//! the Params/Headers cells, the raw body and the Auth fields follow in a later wiring
//! step, each wrapping its input the same way.
//!
//! Dismissal: the popover closes when the pointer moves over the field outside every
//! placeholder, when a move lands outside the field and the popover — the window-level
//! mouse-move watch below, since GPUI has no element-level "mouse left" event — when
//! the field's text changes, when the scopes change and when the mouse leaves the
//! window. The popover surface itself is one of the two places the pointer keeps it
//! open, so its reveal toggle stays clickable.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use api_model::{MASK, VariableKind};
use gpui::{
    Anchor, AnyElement, AnyView, Bounds, Context, DispatchPhase, ElementId, Entity, FocusHandle,
    Focusable, InteractiveElement, IntoElement, MouseExitEvent, MouseMoveEvent, ParentElement,
    Pixels, Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription, Task,
    Window, anchored, canvas, deferred,
};
use http_engine::SecretKeyLookup;
use secrets::{SecretError, SecretKey, SecretStore, SecretValue};
use text_input::{TextInput, TextInputEvent};
use ui::prelude::*;
use variables::{DynamicValues, ScopeKind, ScopeStack, spans};

use crate::RequestEditor;

/// Test hook: the field-sized wrapper the hover hit-tests, what a test hovers over.
pub const VARIABLE_HOVER_TARGET_SELECTOR: &str = "variable-hover-target";

/// What a secret's cell shows while its keychain lookup is still in flight: the answer
/// is security-relevant, so the cell says "unknown yet" rather than guessing "unset"
/// (the quick-look's label).
const CHECKING_LABEL: &str = "…";

/// A secret with no keychain entry: REQ-VAR-SHARE-02's "unresolved secret" marker —
/// a label, never the mask (the quick-look's label).
const UNSET_LABEL: &str = "unset";

/// What the value line shows when a bound variable resolves to no value at all (the
/// quick-look's label).
const NO_VALUE_LABEL: &str = "—";

/// What the value line shows when nothing resolves the hovered name: says so, and
/// carries no value (REQ-VAR-03).
const UNRESOLVED_LABEL: &str = "Not resolved";

/// What the value line shows for a dynamic name (`{{$guid}}`, REQ-VAR-05): generated
/// once per send, never a stored value.
const DYNAMIC_LABEL: &str = "Generated at send";

/// The scope line for a name no scope resolves.
const UNRESOLVED_SCOPE_LABEL: &str = "—";

/// The scope line for a dynamic name: its "scope" is the generator, not a scope stack.
const DYNAMIC_SCOPE_LABEL: &str = "Dynamic";

/// The prefix every keychain failure is shown with, so the inline error says what
/// happened and where (the quick-look's prefix).
const READ_FAILURE_PREFIX: &str = "Could not read this secret — ";

/// The width of the popover surface in pixels at the default scale — room for a long
/// variable name and value; a longer one truncates.
const POPOVER_WIDTH_PX: f32 = 320.;

/// What the editor has been given about where hovered secrets live: the store the
/// reveal reads and [`SecretKeyLookup`] (`http_engine::prepare`'s shared boundary —
/// only the caller knows the workspace and scope ids). The default has neither — a store that has
/// nothing and a key for nothing — so a secret hovers as "unset" and no code path can
/// reach the OS keychain before the app wires its stores (a keychain read can prompt,
/// macOS). The later wiring step fills both through the same setter. `Clone` because
/// two consumers share it: the editor keeps one copy for the send (`send`'s
/// `SendInput`, S33e) and hands the hover its own.
#[derive(Clone)]
pub struct SecretSources {
    /// The keychain the reveal's background task reads.
    pub store: Arc<dyn SecretStore>,
    /// Names one secret's keychain entry from the winning variable's scope kind and name.
    pub key_of: SecretKeyLookup,
}

impl Default for SecretSources {
    fn default() -> Self {
        Self {
            store: Arc::new(NoStoredSecrets),
            key_of: Arc::new(|_, _| None),
        }
    }
}

/// The one "a store that has nothing" store (#193): [`SecretSources`]' default and
/// `headers`' auto-header preview both resolve against this single definition. Nothing
/// can be stored in it and nothing can be read from it, so a hover through it can only
/// ever say "unset" and the preview can never reach the OS keychain (a keychain read
/// can prompt, macOS).
pub(crate) struct NoStoredSecrets;

impl SecretStore for NoStoredSecrets {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        Err(SecretError::NotFound { key: key.clone() })
    }

    fn set(&self, _key: &SecretKey, _value: &SecretValue) -> Result<(), SecretError> {
        Err(SecretError::BackendUnavailable {
            reason: String::from("this store is read-only and stores no secrets"),
        })
    }

    /// Deleting a key that has no secret succeeds ([`SecretStore`]'s own contract).
    fn delete(&self, _key: &SecretKey) -> Result<(), SecretError> {
        Ok(())
    }
}

/// What one hover shows and why — exactly the three strings the popover draws, so a
/// test asserts on the display without re-deriving it (the quick-look's
/// `DisplayedRow` shape). Carries no `Debug` on purpose: [`HoverDisplay::value`] can
/// hold a revealed secret, and debug strings must never hold one.
pub struct HoverDisplay {
    pub name: String,
    pub scope: String,
    pub value: String,
}

/// What a secret's value cell shows and why (the quick-look's `LocalValueDisplay`
/// shape).
enum SecretDisplay {
    /// The keychain lookup has not answered yet ([`CHECKING_LABEL`]).
    Checking,
    /// A secret is set in the keychain: the mask until the user reveals it.
    Masked,
    /// No keychain entry, or no key the store can be asked with ([`UNSET_LABEL`]).
    Unset,
    /// A revealed secret: the value the reveal's lookup fetched. Held only while the
    /// hover is revealed; hiding it drops the value.
    Revealed(SecretValue),
    /// The lookup failed for a reason other than "not found": shown inline, never
    /// disguised as "unset".
    Error(String),
}

/// What the hovered name resolves to.
enum HoverValue {
    /// A plain variable's value, or [`NO_VALUE_LABEL`] when it resolves to nothing.
    Value(Option<String>),
    /// Nothing resolves this name ([`UNRESOLVED_LABEL`]).
    Unresolved,
    /// A dynamic name: generated once per send ([`DYNAMIC_LABEL`], REQ-VAR-05).
    Dynamic,
    /// A secret: masked until the reveal's lookup lands.
    Secret(SecretDisplay),
}

/// One hovered placeholder: the name inside the braces, the scope kind it resolves
/// from and what its value line shows.
struct HoveredVariable {
    name: String,
    /// The winning scope kind's label — shadowed names show the winning scope only.
    scope: String,
    /// The winning scope kind, for the reveal's keychain key. [`None`] for names no
    /// scope holds.
    scope_kind: Option<ScopeKind>,
    value: HoverValue,
}

impl HoveredVariable {
    /// The exact strings the popover draws.
    fn display(&self) -> HoverDisplay {
        HoverDisplay {
            name: self.name.clone(),
            scope: self.scope.clone(),
            value: match &self.value {
                HoverValue::Value(Some(value)) => value.clone(),
                HoverValue::Value(None) => NO_VALUE_LABEL.to_string(),
                HoverValue::Unresolved => UNRESOLVED_LABEL.to_string(),
                HoverValue::Dynamic => DYNAMIC_LABEL.to_string(),
                HoverValue::Secret(display) => match display {
                    SecretDisplay::Checking => CHECKING_LABEL.to_string(),
                    SecretDisplay::Masked => MASK.to_string(),
                    SecretDisplay::Unset => UNSET_LABEL.to_string(),
                    SecretDisplay::Revealed(value) => value.expose().to_string(),
                    SecretDisplay::Error(message) => message.clone(),
                },
            },
        }
    }

    /// Whether the reveal toggle is a live control: only a secret that is set (masked
    /// or already revealed) has a value to reveal.
    fn revealable(&self) -> bool {
        matches!(
            self.value,
            HoverValue::Secret(SecretDisplay::Masked | SecretDisplay::Revealed(_))
        )
    }
}

/// The `{{variable}}` hover behind one field: the placeholder under the pointer
/// ([`text_input`] hit-testing), what it resolves to across the scopes in effect, and
/// the popover showing it.
pub struct VariableHover {
    /// The field whose text is hit-tested — the one whose placeholders hover.
    input: Entity<TextInput>,
    /// Whatever wraps the field's input in the render tree (`url_bar` passes the
    /// autocomplete): this hover is the one attach point over the field.
    wrapped: AnyView,
    /// The scopes the names resolve against (REQ-VAR-03) — the editor's stack, shared
    /// the same way `variable_highlight` and `variable_autocomplete` share it.
    scopes: Arc<ScopeStack>,
    /// Where hovered secrets live ([`SecretSources`]); the default can read nothing.
    sources: SecretSources,
    /// The placeholder under the pointer, while one is hovered.
    hovered: Option<HoveredVariable>,
    /// The open-time status lookup. Stored rather than detached so a hover that goes
    /// away drops — and so cancels — a lookup still running (AGENTS.md › Dropping a
    /// `Task` cancels it).
    status_lookup: Option<Task<()>>,
    /// The in-flight reveal lookup. Hiding the value cancels its fetch: a hidden hover
    /// is never fetched back into the view.
    reveal_lookup: Option<Task<()>>,
    /// The reveal toggle's focus handle: what makes the toggle a tab stop of the open
    /// popover and what the keyboard reaches it with.
    reveal_focus: FocusHandle,
    /// The popover surface's bounds as the last frame painted them, for the
    /// pointer-leave watch below (the `text_input` `last_bounds` shape: paint-time
    /// geometry through interior mutability, never `Entity::update` from
    /// `render`/`paint`). [`None`] while no popover is painted.
    popover_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    // Kept so the field's edits keep reaching us; dropping it would silently stop the
    // hover from closing when the text under it changes.
    _input_subscription: Subscription,
}

impl VariableHover {
    /// The hover behind `input`, closed, with `wrapped` rendered through it.
    pub fn new(
        input: Entity<TextInput>,
        wrapped: AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        Self {
            input,
            wrapped,
            scopes: Arc::new(ScopeStack::new()),
            sources: SecretSources::default(),
            hovered: None,
            status_lookup: None,
            reveal_lookup: None,
            reveal_focus: cx.focus_handle(),
            popover_bounds: Rc::new(Cell::new(None)),
            _input_subscription: subscription,
        }
    }

    /// Store the scopes the hover resolves against and close any open hover — its
    /// value and scope may just have changed. The same `set_scopes` shape as
    /// `variable_highlight` and `variable_autocomplete`.
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.scopes = scopes;
        self.close(cx);
    }

    /// Store where hovered secrets live ([`SecretSources`]) and close any open hover —
    /// its masked state was answered by the previous sources.
    pub fn set_secret_sources(&mut self, sources: SecretSources, cx: &mut Context<Self>) {
        self.sources = sources;
        self.close(cx);
    }

    /// The hover as the user sees it, exactly as the popover draws it — what a test
    /// reads it by. [`None`] while no placeholder is hovered.
    pub fn hovered(&self) -> Option<HoverDisplay> {
        self.hovered.as_ref().map(HoveredVariable::display)
    }

    /// The reveal toggle, as the keyboard reaches it: what a test (and later wiring)
    /// hands the control its focus with.
    pub fn reveal_focus_handle(&self) -> FocusHandle {
        self.reveal_focus.clone()
    }

    /// The pointer moved over the field: open the hover on the placeholder under it,
    /// keep the one already open on the same name, and close on empty text.
    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.input.read(cx).index_for_mouse_position(event.position);
        let text = self.input.read(cx).text();
        let hovered_name = spans(&text, &self.scopes)
            .into_iter()
            .find(|span| span.range.contains(&offset))
            .map(|span| span.name);
        match hovered_name {
            Some(name) => {
                if self.hovered.as_ref().map(|hovered| hovered.name.as_str()) != Some(name.as_str())
                {
                    self.open_hover(&name, cx);
                }
            }
            None => self.close(cx),
        }
    }

    /// The mouse left the window: nothing is hovered any more.
    fn on_mouse_exit(&mut self, _: &MouseExitEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
    }

    /// The field's text changed under the pointer: the spans it hovers over are gone,
    /// so the hover goes with them (recomputed on the next move).
    fn on_input_event(
        &mut self,
        _input: &Entity<TextInput>,
        event: &TextInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event == TextInputEvent::Changed {
            self.close(cx);
        }
    }

    /// Open the hover on `name`, dropping whatever was open and any lookup it had in
    /// flight. A secret starts [`SecretDisplay::Checking`] and its status lookup runs
    /// on a background task (the quick-look's open-time shape).
    fn open_hover(&mut self, name: &str, cx: &mut Context<Self>) {
        self.close(cx);
        let hovered = self.hovered_variable(name);
        if matches!(hovered.value, HoverValue::Secret(_)) {
            self.status_lookup = Some(self.start_status_lookup(&hovered, cx));
        }
        self.hovered = Some(hovered);
        self.reveal_focus.clone().tab_stop(true);
        cx.notify();
    }

    /// What `name` hovers as: the winning binding's scope kind and value, a secret
    /// masked, a dynamic name generated at send, or the unresolved label.
    fn hovered_variable(&self, name: &str) -> HoveredVariable {
        if DynamicValues::is_dynamic_name(name) {
            return HoveredVariable {
                name: name.to_string(),
                scope: DYNAMIC_SCOPE_LABEL.to_string(),
                scope_kind: None,
                value: HoverValue::Dynamic,
            };
        }
        match self.scopes.find(name) {
            Some(binding) => {
                let scope_kind = scope_kind_of(&self.scopes, name);
                let value = if binding.variable.kind == VariableKind::Secret {
                    // A secret's value is never read here, even when the stack carries
                    // a local one (REQ-VAR-SHARE-02): the reveal's lookup is the one
                    // door to a secret's plaintext.
                    HoverValue::Secret(SecretDisplay::Checking)
                } else {
                    HoverValue::Value(binding.value.map(str::to_string))
                };
                HoveredVariable {
                    name: name.to_string(),
                    scope: scope_label(scope_kind),
                    scope_kind,
                    value,
                }
            }
            None => HoveredVariable {
                name: name.to_string(),
                scope: UNRESOLVED_SCOPE_LABEL.to_string(),
                scope_kind: None,
                value: HoverValue::Unresolved,
            },
        }
    }

    /// Ask the keychain, off the foreground thread, whether the hovered secret has an
    /// entry. Only the answer crosses back — the value is dropped inside the lookup,
    /// so the mask's "something is set here" claim costs no plaintext.
    fn start_status_lookup(&self, hovered: &HoveredVariable, cx: &mut Context<Self>) -> Task<()> {
        let name = hovered.name.clone();
        let scope_kind = hovered.scope_kind;
        let store = Arc::clone(&self.sources.store);
        let key_of = Arc::clone(&self.sources.key_of);
        let lookup_name = name.clone();
        cx.spawn(async move |this, cx| {
            let display = cx
                .background_executor()
                .spawn(async move {
                    match lookup_secret(&store, &key_of, scope_kind, &lookup_name) {
                        SecretDisplay::Revealed(_value) => SecretDisplay::Masked,
                        other => other,
                    }
                })
                .await;
            if this
                .update(cx, |this, cx| {
                    this.status_lookup = None;
                    this.apply_secret_display(&name, display);
                    cx.notify();
                })
                .is_err()
            {
                // Handled: the hover went away with its window; its state is gone.
            }
        })
    }

    /// Turn the reveal toggle: on runs the reveal's keychain lookup on a background
    /// task, off drops the value back to the mask and cancels any fetch that has not
    /// landed yet (the quick-look's shape).
    fn set_revealed(&mut self, revealed: bool, cx: &mut Context<Self>) {
        if !revealed {
            self.reveal_lookup = None;
            if let Some(hovered) = &mut self.hovered
                && matches!(
                    hovered.value,
                    HoverValue::Secret(SecretDisplay::Revealed(_))
                )
            {
                hovered.value = HoverValue::Secret(SecretDisplay::Masked);
            }
            cx.notify();
            return;
        }
        let Some(hovered) = &mut self.hovered else {
            return;
        };
        // While the fetch is in flight the cell says so and the toggle stays off: the
        // reveal is not on until the value is on screen.
        hovered.value = HoverValue::Secret(SecretDisplay::Checking);
        let name = hovered.name.clone();
        let scope_kind = hovered.scope_kind;
        let store = Arc::clone(&self.sources.store);
        let key_of = Arc::clone(&self.sources.key_of);
        let lookup_name = name.clone();
        self.reveal_lookup = Some(cx.spawn(async move |this, cx| {
            let display = cx
                .background_executor()
                .spawn(async move { lookup_secret(&store, &key_of, scope_kind, &lookup_name) })
                .await;
            if this
                .update(cx, |this, cx| {
                    this.reveal_lookup = None;
                    this.apply_secret_display(&name, display);
                    cx.notify();
                })
                .is_err()
            {
                // Handled: the hover went away with its window; the fetched value
                // goes with it.
            }
        }));
        cx.notify();
    }

    /// Put `display` in the hover's value line, if `name` is still the hovered secret.
    fn apply_secret_display(&mut self, name: &str, display: SecretDisplay) {
        if let Some(hovered) = &mut self.hovered
            && hovered.name == name
            && matches!(hovered.value, HoverValue::Secret(_))
        {
            hovered.value = HoverValue::Secret(display);
        }
    }

    /// Close the hover, dropping the value and cancelling any lookup still running.
    fn close(&mut self, cx: &mut Context<Self>) {
        if self.hovered.is_none() {
            return;
        }
        self.hovered = None;
        self.popover_bounds.set(None);
        self.status_lookup = None;
        self.reveal_lookup = None;
        // The toggle is out of the render tree now; it must leave the tab order with it.
        self.reveal_focus.clone().tab_stop(false);
        cx.notify();
    }

    /// The popover under the field: the anchored floating surface (the
    /// `variable_autocomplete`'s shape) holding the name, the scope and the value —
    /// with the reveal toggle on a secret. [`None`] while nothing is hovered.
    fn render_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let hovered = self.hovered.as_ref()?;
        let display = hovered.display();
        // Every element id in this popover derives from the field's focus handle — the
        // control's identity — so two fields can never collide in one frame (the
        // `variable_autocomplete`'s rule), and the child ids come from the variable's
        // name (stable: never a position in a list).
        let base_id = ElementId::from(&self.input.read(cx).focus_handle(cx));
        let surface_id = ElementId::NamedChild(
            Arc::new(base_id.clone()),
            format!("hover-{}", display.name).into(),
        );
        let value_row = h_flex()
            .min_w_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(self.render_value(&base_id, &display, hovered))
            .children(self.render_reveal(&display, hovered, cx));
        let surface = v_flex()
            .id(surface_id)
            .role(Role::Tooltip)
            .aria_label(SharedString::new_static("Variable value"))
            .elevation_2(cx)
            .rounded_sm()
            .p(DynamicSpacing::Base04.rems(cx))
            .w(px_to_rems(POPOVER_WIDTH_PX))
            .max_w_full()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(
                Label::new(display.name.clone())
                    .size(LabelSize::Small)
                    .truncate(),
            )
            .child(
                Label::new(display.scope.clone())
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .child(value_row)
            .child(self.render_popover_bounds());
        Some(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                            .child(surface),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }

    /// The value line's text: truncating, with the full value in the tooltip for a
    /// plain value — and no tooltip on a secret in any state (AGENTS.md › Security).
    fn render_value(
        &self,
        base_id: &ElementId,
        display: &HoverDisplay,
        hovered: &HoveredVariable,
    ) -> AnyElement {
        let text = display.value.clone();
        let cell = div()
            .id(ElementId::NamedChild(
                Arc::new(base_id.clone()),
                format!("value-{}", display.name).into(),
            ))
            .min_w_0()
            .flex_1()
            .child(
                Label::new(text.clone())
                    .size(LabelSize::Small)
                    .color(Color::Default)
                    .truncate(),
            );
        match hovered.value {
            HoverValue::Value(Some(_)) => cell
                .tooltip(Tooltip::text(text).builder())
                .into_any_element(),
            _ => cell.into_any_element(),
        }
    }

    /// The reveal toggle on a secret's value: the keyboard-reachable control whose
    /// label is its meaning and whose id is the variable's name (the quick-look's row
    /// shape). [`None`] for a name that is not a secret.
    fn render_reveal(
        &self,
        display: &HoverDisplay,
        hovered: &HoveredVariable,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !matches!(hovered.value, HoverValue::Secret(_)) {
            return None;
        }
        let base_id = ElementId::from(&self.input.read(cx).focus_handle(cx));
        let toggle_id =
            ElementId::NamedChild(Arc::new(base_id), format!("reveal-{}", display.name).into());
        let state = if matches!(
            hovered.value,
            HoverValue::Secret(SecretDisplay::Revealed(_))
        ) {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        };
        let this = cx.weak_entity();
        Some(
            Toggle::new(toggle_id, SharedString::new_static("Reveal"), state)
                .focus_handle(self.reveal_focus.clone())
                .disabled(!hovered.revealable())
                .on_change(move |toggle_state, _window, cx| {
                    if this
                        .update(cx, |this, cx| {
                            this.set_revealed(toggle_state.is_selected(), cx)
                        })
                        .is_err()
                    {
                        // Handled: the hover went away between click and callback;
                        // there is nothing left to reveal.
                    }
                })
                .into_any_element(),
        )
    }
    /// The popover surface's bounds, learned while painting: the pointer-leave watch
    /// keeps the hover open while the pointer is anywhere on the surface — its reveal
    /// toggle must stay clickable (REQ-VAR-03).
    fn render_popover_bounds(&self) -> AnyElement {
        let popover_bounds = Rc::clone(&self.popover_bounds);
        canvas(
            |_, _, _| {},
            move |bounds, _, _, _| {
                popover_bounds.set(Some(bounds));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    /// The window-wide pointer watch behind the pointer-leave dismissal (REQ-VAR-03):
    /// GPUI dispatches [`MouseMoveEvent`]s only to the element under the pointer, so a
    /// move outside the field never reaches [`Self::on_mouse_move`] and the popover
    /// used to stay open until the pointer came back. A [`canvas`] paint hook is the
    /// one place that both sees the field's own bounds and may register a window-level
    /// listener — the shape `response_viewer::body_view` watches a drag beyond its rows
    /// with. The hover stays open for a move over the field or over the popover
    /// surface; anything else dismisses it.
    fn render_pointer_watch(&self, cx: &mut Context<Self>) -> AnyElement {
        let hover = cx.weak_entity();
        let popover_bounds = Rc::clone(&self.popover_bounds);
        canvas(
            |_, _, _| {},
            move |field_bounds, _, window, _| {
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let over_popover = popover_bounds
                        .get()
                        .is_some_and(|popover| popover.contains(&event.position));
                    if field_bounds.contains(&event.position) || over_popover {
                        return;
                    }
                    if hover.update(cx, |this, cx| this.close(cx)).is_err() {
                        // Handled: the hover went away with its window; there is
                        // nothing left to close.
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }
}

impl Render for VariableHover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .debug_selector(|| VARIABLE_HOVER_TARGET_SELECTOR.to_string())
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_exit(cx.listener(Self::on_mouse_exit))
            .child(self.wrapped.clone())
            .children(self.render_popover(cx))
            .child(self.render_pointer_watch(cx))
    }
}

impl RequestEditor {
    /// Where a hovered secret's value comes from (REQ-VAR-03): the store the reveal
    /// reads and the one door to its keychain key ([`SecretSources`]). The later wiring
    /// step provides the workspace and scope ids the same way. Until this is called
    /// nothing can reach the OS keychain and a secret hovers as "unset". The editor
    /// keeps its own copy of the sources for the send (`send`'s `SendInput`, S33e) and
    /// hands the hover the one it reads.
    pub fn set_secret_sources(&mut self, sources: SecretSources, cx: &mut Context<Self>) {
        self.secret_sources = sources.clone();
        let hover = self.url_bar.read(cx).hover().clone();
        hover.update(cx, |hover, cx| hover.set_secret_sources(sources, cx));
    }
}

/// The scope kind the winning variable of `name` lives in — the keychain key names a
/// secret by scope. [`ScopeStack::find`] does not return the scope, so this walks the
/// same precedence rule it does: the narrowest scope holding an enabled `name` wins.
fn scope_kind_of(scopes: &ScopeStack, name: &str) -> Option<ScopeKind> {
    scopes
        .scopes()
        .iter()
        .find(|scope| {
            scope
                .variables
                .iter()
                .any(|variable| variable.enabled && variable.key == name)
        })
        .map(|scope| scope.kind)
}

/// How one scope kind is named on the hover's scope line (REQ-VAR-03: the scope it
/// came from).
fn scope_label(scope_kind: Option<ScopeKind>) -> String {
    match scope_kind {
        Some(ScopeKind::RequestLocal) => String::from("Request-local"),
        Some(ScopeKind::Data) => String::from("Data"),
        Some(ScopeKind::Environment) => String::from("Environment"),
        Some(ScopeKind::Collection) => String::from("Collection"),
        Some(ScopeKind::Global) => String::from("Global"),
        None => UNRESOLVED_SCOPE_LABEL.to_string(),
    }
}

/// One keychain lookup behind a hover: the value the reveal shows, or why there is
/// none. Runs off the foreground thread. The key comes from the caller's
/// [`SecretKeyLookup`] (`http_engine::prepare`'s boundary); nothing here logs a value and a
/// [`SecretError`]'s text names keys, never values.
fn lookup_secret(
    store: &Arc<dyn SecretStore>,
    key_of: &SecretKeyLookup,
    scope_kind: Option<ScopeKind>,
    name: &str,
) -> SecretDisplay {
    let Some(key) = scope_kind.and_then(|kind| key_of(kind, name)) else {
        return SecretDisplay::Unset;
    };
    match store.get(&key) {
        Ok(value) => SecretDisplay::Revealed(value),
        Err(SecretError::NotFound { .. }) => SecretDisplay::Unset,
        Err(error) => SecretDisplay::Error(read_failure(&error)),
    }
}

/// The inline error for a keychain failure: what happened and on which key, so the
/// message is actionable (AGENTS.md › Errors reach the user; the quick-look's shape).
/// The error names the key, never a value.
fn read_failure(error: &SecretError) -> String {
    format!("{READ_FAILURE_PREFIX}{error}")
}

#[cfg(test)]
#[path = "variable_hover_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "variable_hover_pointer_leave_tests.rs"]
mod pointer_leave_tests;
