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
//! S48u6 (mockup section 7) turns the hover into the place a variable is fixed: a
//! plain value resolved from a scope the app's wire can write is an editable field
//! (Enter or Save lands it through the wire's [`VariableWrites`] — the variables
//! editor's own save path), a secret keeps the mask and offers "Edit secret…"
//! (`environment::EditSecret`), and an undefined name says "<name> is not defined" with
//! "Add to <active environment>" and "Add to Globals". The scope line carries a badge
//! letter — E, G or C — and the scope's own name ("Environment · Development"). The
//! popover also opens from the keyboard: with the field focused, a caret inside a
//! `{{placeholder}}` opens it, and leaving the placeholder closes it.
//!
//! `Decision:` "All variables in this request →" is omitted: the variable line right
//! under the URL bar already lists every variable the request uses, so the link would go
//! nowhere new.
//!
//! `Decision:` the secret's reveal toggle (REQ-VAR-03, S33) stays — it is the user's
//! explicit request to see their own secret — but nothing reveals by default: the mask
//! is what the popover draws, and no tooltip, element id, toast or log carries a value.
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
    Anchor, AnyElement, AnyView, App, Bounds, Context, DispatchPhase, ElementId, Entity,
    FocusHandle, Focusable, InteractiveElement, IntoElement, MouseExitEvent, MouseMoveEvent,
    ParentElement, Pixels, Render, Role, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, anchored, canvas, deferred,
};
use http_engine::SecretKeyLookup;
use pustman_actions::environment::EditSecret;
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

/// The placeholder the editable value field shows while it is empty.
const VALUE_PLACEHOLDER: &str = "Value";

/// The buttons the popover can carry, by what they do. Their ids derive from these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverAction {
    /// Land the value field's text in the variable's scope.
    Save,
    /// Open the variables editor on the secret's masked field.
    EditSecret,
    /// Add the undefined name to this scope.
    AddTo(ScopeKind),
}

/// Lands one edit from the popover: the app's wire maps the scope kind to the scope the
/// request resolves it from and writes through the variables editor's save path
/// (`environment_picker::env_editor::write`), reporting the outcome itself.
pub type VariableWriter = Arc<dyn Fn(VariableEdit, &mut Window, &mut App)>;

/// One edit the popover asks the wire to land (S48u6). Never a secret: the popover has
/// no field for one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableEdit {
    /// The scope kind the variable resolves from — or, for an undefined name, the one it
    /// is added to.
    pub scope: ScopeKind,
    pub name: String,
    pub value: String,
    /// The request resolved the value from the scope's local value, so the edit sets that
    /// one (the shared value otherwise).
    pub local: bool,
}

/// Where the popover's edits land (S48u6): the name of each scope the request resolves
/// from — what the scope line and the "Add to …" buttons say — and the writer. The
/// default writes nowhere: values are read-only and an undefined name offers no Add,
/// until the app's wire provides both.
#[derive(Clone, Default)]
pub struct VariableWrites {
    /// Each writable scope kind with its scope's name ("Development", "Globals", a
    /// collection's name).
    pub scope_names: Vec<(ScopeKind, String)>,
    pub write: Option<VariableWriter>,
}

impl VariableWrites {
    fn scope_name(&self, kind: ScopeKind) -> Option<&str> {
        self.scope_names
            .iter()
            .find(|(scope, _)| *scope == kind)
            .map(|(_, name)| name.as_str())
    }

    /// Whether an edit to `kind` can land.
    fn writable(&self, kind: ScopeKind) -> bool {
        self.write.is_some() && self.scope_name(kind).is_some()
    }
}

/// What the popover draws, line by line — the one model `render` and
/// [`VariableHover::rendered_texts`] both read, so a test of the text is a test of the
/// screen.
struct PopoverModel {
    name: String,
    /// E, G or C for an environment, the globals or a collection.
    badge: Option<&'static str>,
    scope_line: String,
    value: ValueLine,
    buttons: Vec<(PopoverAction, String)>,
}

/// The popover's value line.
enum ValueLine {
    /// The read-only value text (the masked, unset, checking or dynamic labels too).
    Text(String),
    /// The editable value field.
    Field,
    /// "<name> is not defined".
    NotDefined(String),
}

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
    /// The editable value field of a plain variable (S48u6).
    value_input: Entity<TextInput>,
    /// Where the popover's edits land ([`VariableWrites`]); the default writes nowhere.
    writes: VariableWrites,
    /// The popover was opened by the caret, not the pointer: moving the caret out of the
    /// placeholder closes it.
    opened_by_caret: bool,
    // Kept so the field's edits keep reaching us; dropping it would silently stop the
    // hover from closing when the text under it changes.
    _input_subscription: Subscription,
    // The caret watch and the value field's Enter; dropping them stops both.
    _subscriptions: Vec<Subscription>,
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
        let value_input = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(SharedString::new_static(VALUE_PLACEHOLDER))
        });
        let subscriptions = vec![
            cx.observe_in(&input, window, |this, _input, window, cx| {
                this.follow_caret(window, cx)
            }),
            cx.subscribe_in(&value_input, window, |this, _field, event, window, cx| {
                if *event == TextInputEvent::Submitted {
                    this.run_action(PopoverAction::Save, window, cx);
                }
            }),
        ];
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
            value_input,
            writes: VariableWrites::default(),
            opened_by_caret: false,
            _input_subscription: subscription,
            _subscriptions: subscriptions,
        }
    }

    /// Store where the popover's edits land ([`VariableWrites`]) and close any open
    /// hover — what it offered was answered by the previous writes.
    pub fn set_variable_writes(&mut self, writes: VariableWrites, cx: &mut Context<Self>) {
        self.writes = writes;
        self.close(cx);
    }

    /// The editable value field, as the keyboard and a test reach it.
    pub fn value_input(&self) -> &Entity<TextInput> {
        &self.value_input
    }

    /// Every string the open popover draws, in order — the name, the scope badge and
    /// line, the value (an editable field's own masked-or-plain display text), the
    /// "not defined" line and each button's label. Empty while nothing is hovered.
    pub fn rendered_texts(&self, cx: &App) -> Vec<String> {
        let Some(model) = self.popover_model() else {
            return Vec::new();
        };
        let mut texts = vec![model.name];
        texts.extend(model.badge.map(str::to_string));
        if !model.scope_line.is_empty() {
            texts.push(model.scope_line);
        }
        match model.value {
            ValueLine::Text(text) | ValueLine::NotDefined(text) => texts.push(text),
            ValueLine::Field => texts.push(self.value_input.read(cx).display_text()),
        }
        if self
            .hovered
            .as_ref()
            .is_some_and(|hovered| matches!(hovered.value, HoverValue::Secret(_)))
        {
            texts.push(String::from("Reveal"));
        }
        texts.extend(model.buttons.into_iter().map(|(_, label)| label));
        texts
    }

    /// The caret moved in the field (the keyboard's way in): inside a placeholder it
    /// opens the popover on it; out of every placeholder it closes a popover the caret
    /// opened. A blurred field changes nothing — focus may have moved into the popover.
    fn follow_caret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        if !input.focus_handle(cx).is_focused(window) {
            return;
        }
        let selection = input.selected_range();
        if !selection.is_empty() {
            return;
        }
        let caret = selection.start;
        let text = input.text();
        let name = spans(&text, &self.scopes)
            .into_iter()
            .find(|span| span.range.start < caret && caret < span.range.end)
            .map(|span| span.name);
        match name {
            Some(name) => {
                if self.hovered.as_ref().map(|hovered| hovered.name.as_str()) != Some(name.as_str())
                {
                    self.open_hover(&name, cx);
                    self.opened_by_caret = true;
                }
            }
            None => {
                if self.opened_by_caret {
                    self.close(cx);
                }
            }
        }
    }

    /// The popover's model for what is hovered (see [`PopoverModel`]).
    fn popover_model(&self) -> Option<PopoverModel> {
        let hovered = self.hovered.as_ref()?;
        let display = hovered.display();
        let scope_kind = hovered.scope_kind;
        let badge = scope_kind.and_then(scope_badge);
        let scope_line = match scope_kind
            .and_then(|kind| self.writes.scope_name(kind).map(|name| (kind, name)))
        {
            Some((ScopeKind::Global, _)) => String::from("Globals"),
            Some((kind, name)) => format!("{} · {name}", scope_label(Some(kind))),
            // An undefined name comes from no scope: its "not defined" line says so, and
            // a scope line of "—" would only repeat it.
            None if matches!(hovered.value, HoverValue::Unresolved) => String::new(),
            None => display.scope.clone(),
        };
        let mut buttons = Vec::new();
        let value = match &hovered.value {
            HoverValue::Value(_) if scope_kind.is_some_and(|kind| self.writes.writable(kind)) => {
                buttons.push((PopoverAction::Save, String::from("Save")));
                ValueLine::Field
            }
            HoverValue::Secret(_) => {
                buttons.push((PopoverAction::EditSecret, String::from("Edit secret…")));
                ValueLine::Text(display.value.clone())
            }
            HoverValue::Unresolved => {
                for kind in [ScopeKind::Environment, ScopeKind::Global] {
                    if !self.writes.writable(kind) {
                        continue;
                    }
                    let label = match (kind, self.writes.scope_name(kind)) {
                        (ScopeKind::Environment, Some(name)) => format!("Add to {name}"),
                        _ => String::from("Add to Globals"),
                    };
                    buttons.push((PopoverAction::AddTo(kind), label));
                }
                ValueLine::NotDefined(format!("{} is not defined", display.name))
            }
            _ => ValueLine::Text(display.value.clone()),
        };
        Some(PopoverModel {
            name: display.name,
            badge,
            scope_line,
            value,
            buttons,
        })
    }

    /// Run one of the popover's buttons (or Enter in its value field), then close it.
    fn run_action(&mut self, action: PopoverAction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hovered) = self.hovered.as_ref() else {
            return;
        };
        let name = hovered.name.clone();
        let edit = match action {
            PopoverAction::EditSecret => {
                window.dispatch_action(Box::new(EditSecret { name }), cx);
                self.close(cx);
                return;
            }
            PopoverAction::Save => {
                let Some(scope) = hovered.scope_kind else {
                    return;
                };
                if !matches!(hovered.value, HoverValue::Value(_)) {
                    // Only a plain value has a field; a secret is never written here.
                    return;
                }
                VariableEdit {
                    scope,
                    local: self.scopes.local_value(&name).is_some(),
                    value: self.value_input.read(cx).text(),
                    name,
                }
            }
            PopoverAction::AddTo(scope) => VariableEdit {
                scope,
                name,
                value: String::new(),
                local: false,
            },
        };
        let Some(write) = self.writes.write.clone() else {
            return;
        };
        self.close(cx);
        write(edit, window, cx);
    }

    /// Store the scopes the hover resolves against and close any open hover — its
    /// value and scope may just have changed. The same `set_scopes` shape as
    /// `variable_highlight` and `variable_autocomplete`.
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.scopes = scopes;
        self.close(cx);
    }

    /// Store where hovered secrets live ([`SecretSources`]) and close any open hover —
    /// its masked state was answered by the previous sources. The pre-send check
    /// re-runs too: the sources decide which secret names the send resolves (#198).
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
        if let HoverValue::Value(value) = &hovered.value {
            let text = value.clone().unwrap_or_default();
            self.value_input
                .update(cx, |field, cx| field.set_text(&text, cx));
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
        self.opened_by_caret = false;
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
        let model = self.popover_model()?;
        let value_row = h_flex()
            .min_w_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(match &model.value {
                ValueLine::Field => div()
                    .flex_1()
                    .min_w_0()
                    .px(DynamicSpacing::Base04.rems(cx))
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_sm()
                    .font_buffer(cx)
                    .child(self.value_input.clone())
                    .into_any_element(),
                ValueLine::NotDefined(text) => div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        Label::new(text.clone())
                            .size(LabelSize::Small)
                            .color(Color::Error)
                            .truncate(),
                    )
                    .into_any_element(),
                ValueLine::Text(_) => self.render_value(&base_id, &display, hovered),
            })
            .children(self.render_reveal(&display, hovered, cx));
        let scope_row = h_flex()
            .min_w_0()
            .gap(DynamicSpacing::Base03.rems(cx))
            .children(model.badge.map(|letter| {
                div()
                    .flex_none()
                    .px(DynamicSpacing::Base02.rems(cx))
                    .rounded_sm()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .child(
                        Label::new(letter)
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
            }))
            .child(
                div().min_w_0().flex_1().child(
                    Label::new(model.scope_line.clone())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                ),
            );
        let buttons = model
            .buttons
            .iter()
            .enumerate()
            .map(|(position, (action, label))| {
                let id = ElementId::NamedChild(
                    Arc::new(base_id.clone()),
                    format!("{}-{}", action_id(*action), display.name).into(),
                );
                let action = *action;
                let selector = popover_button_selector(action);
                div()
                    .debug_selector(move || selector.to_owned())
                    .child(
                        Button::new(id, label.clone())
                            .size(ButtonSize::Medium)
                            .style(if position == 0 {
                                ButtonStyle::Filled
                            } else {
                                ButtonStyle::Outlined
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.run_action(action, window, cx)
                            })),
                    )
                    .into_any_element()
            });
        let button_row = h_flex()
            .flex_wrap()
            .gap(DynamicSpacing::Base04.rems(cx))
            .children(buttons);
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
            .when(!model.scope_line.is_empty(), |surface| {
                surface.child(scope_row)
            })
            .child(value_row)
            .when(!model.buttons.is_empty(), |surface| {
                surface.child(button_row)
            })
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
    /// Where the `{{var}}` popover's edits land ([`VariableWrites`], S48u6): the app's
    /// scope wire gives every bound editor the names of the scopes its request resolves
    /// from and the writer that saves into them.
    pub fn set_variable_writes(&mut self, writes: VariableWrites, cx: &mut Context<Self>) {
        let hover = self.url_bar.read(cx).hover().clone();
        hover.update(cx, |hover, cx| hover.set_variable_writes(writes, cx));
    }

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
        self.recheck_unresolved(cx);
    }
}

/// The scope kind the winning variable of `name` lives in — the keychain key names a
/// secret by scope. [`ScopeStack::find`] does not return the scope, so this walks the
/// same precedence rule it does: the narrowest scope holding an enabled `name` wins.
pub fn scope_kind_of(scopes: &ScopeStack, name: &str) -> Option<ScopeKind> {
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

/// The scope badge's letter (mockup section 7): E, G or C for the scopes a user edits.
fn scope_badge(kind: ScopeKind) -> Option<&'static str> {
    match kind {
        ScopeKind::Environment => Some("E"),
        ScopeKind::Global => Some("G"),
        ScopeKind::Collection => Some("C"),
        ScopeKind::RequestLocal | ScopeKind::Data => None,
    }
}

/// Test hook: one popover button's debug selector.
pub fn popover_button_selector(action: PopoverAction) -> &'static str {
    match action {
        PopoverAction::Save => "variable-popover-save",
        PopoverAction::EditSecret => "variable-popover-edit-secret",
        PopoverAction::AddTo(ScopeKind::Environment) => "variable-popover-add-environment",
        PopoverAction::AddTo(ScopeKind::Global) => "variable-popover-add-globals",
        PopoverAction::AddTo(_) => "variable-popover-add-scope",
    }
}

/// The stable id part of one popover button.
fn action_id(action: PopoverAction) -> &'static str {
    match action {
        PopoverAction::Save => "save",
        PopoverAction::EditSecret => "edit-secret",
        PopoverAction::AddTo(ScopeKind::Environment) => "add-environment",
        PopoverAction::AddTo(ScopeKind::Global) => "add-globals",
        PopoverAction::AddTo(_) => "add-scope",
    }
}

/// How one scope kind is named on the hover's scope line (REQ-VAR-03: the scope it
/// came from).
pub(crate) fn scope_label(scope_kind: Option<ScopeKind>) -> String {
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

#[cfg(test)]
#[path = "variable_hover_popover_tests.rs"]
mod popover_tests;
