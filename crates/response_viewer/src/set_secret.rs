//! The "Set <name>" flow behind the `UnresolvedSecret` error panel (PRD §8.6): a secret
//! with no value anywhere is one the OS keychain holds nothing for, so the panel offers a
//! "Set <name>" button per missing name. Each dispatches `environment::EditSecret` — the
//! variables editor (`environment_picker`) opens on the scope the wire installed with
//! that secret's masked field under the cursor — and the value is typed and saved *there*,
//! whose module docs hold the save order and every guarantee around it (S43a/S43b).
//! Nothing in this module reads, writes or even names a secret *value*: it names keys.
//!
//! When the editor goes and the send's own resolution says every missing name resolves
//! now, the panel offers **Resend**, which dispatches `request::Send` — after that,
//! re-sending works because the send's keychain read finds what is now stored.
//!
//! `Decision:` Resend's readiness is the *send's* answer, never a guess at scopes (S43c
//! fix 1): the re-check runs [`http_engine::prepare`] — the very resolution the send runs,
//! whose scope rules live in `variables` — over the failed send's own [`SendInput`],
//! carried by [`SetSecretInputs`] (the `cookie_dialog::CookieDialogInputs` shape for "the
//! top crate owns the inputs"), off the foreground. Resend is offered only when
//! `prepare` reports no `UnresolvedSecret` at all — the panel's names are exactly the
//! names the failed send could not resolve, so "none are missing now" is "every
//! originally missing name now resolves". With no inputs installed the re-check says why
//! Resend is not offered instead of returning silently (S43c fix 1, item 3).
//!
//! `Decision:` "on save" is observed, never assumed: the editor emits no save event (its
//! `DismissEvent` also fires on Cancel and on Escape), so the flow watches the workspace's
//! modal layer and, when the modal it saw open goes, re-runs the resolution. A name set
//! through some other path is seen by the same re-check once the flow has watched an
//! editor; until then the request editor's own Send button is the way to re-send.
//!
//! `Decision:` state is per panel and is rebuilt for every new one (S43c fix 1/3): an
//! answer belongs to the question it answers — the panel, its missing names and the send
//! *that panel failed with*. The failed send is captured when the panel is built
//! ([`capture_failed_send`], S43c fix 3) with a monotonic id an [`Arc`] address can never
//! recycle, so two tabs failing on the same name in different scopes never share an
//! answer, neither one's re-check moves the other's, and a later failure — even one
//! naming the same secret in another scope — starts with no offer at all.
//!
//! `Decision:` this crate coordinates with the variables editor through the
//! `environment::EditSecret` action only — never a package edge (`pustman_actions` is the
//! shared vocabulary, and a `response_viewer → environment_picker` dependency would close
//! a package cycle). The editor's own opener reports everything it refuses (its inputs
//! missing, a name its scope has no row for) with its own toasts.
//!
//! `Decision:` a variable name reaches a label, tooltip or note only through
//! [`display_name`] — Unicode control characters and bidi overrides stripped — so a
//! crafted name cannot reorder or hide what those say. The name used for lookup and for
//! the dispatched action is unchanged (S43c fix 1, item 4).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use api_model::Auth;
use gpui::{Action, App, Context, Entity, Global, Subscription, Task, Window};
use http_engine::{PipelineError, SendInput, TransportError, prepare};
use pustman_actions::environment::EditSecret;
use pustman_actions::request::Send as SendRequest;
use ui::prelude::*;
use variables::DynamicValues;
use workspace::Workspace;

use crate::child_id;

/// The debug key the "Set <name>" button renders under — the test hook that lets
/// `VisualTestContext::debug_bounds` find one button per missing name (the
/// [`crate::error_panel::error_kind_selector`] shape). Keyed by the name *as the label
/// shows it* ([`display_name`]), so the key always names the wording on screen (#125's
/// shape): a name whose label is sanitized has no raw-text key to find.
pub fn set_button_selector(label_name: &str) -> String {
    format!("set-secret-button-{label_name}")
}

/// The debug key the Resend button renders under while it is shown (see
/// [`set_button_selector`]).
pub const RESEND_BUTTON_SELECTOR: &str = "set-secret-button-resend";

/// What Resend is offered for: it re-sends the request, which runs the same failure
/// again unless every missing secret resolves.
const RESEND_LABEL: &str = "Resend";

/// What a "Set <name>" button says — the name as the label shows it ([`display_name`]).
pub fn set_label(name: &str) -> String {
    format!("Set {}", display_name(name))
}

/// What its tooltip says — the same sanitized name, never a value.
pub fn set_tooltip(name: &str) -> String {
    format!(
        "Set {}'s secret value in the variables editor",
        display_name(name)
    )
}

/// The row under the `UnresolvedSecret` panel's suggested fix: one "Set <name>" button
/// per missing name, Resend once they all resolve (module docs), and — while they do
/// not — a note saying why Resend is not offered.
#[derive(Clone, Debug, IntoElement)]
pub struct SetSecretRow {
    /// The panel's stable id — every button's element id derives from it
    /// (`.agents/rules/ui-render.md`).
    id: ElementId,
    /// The missing names the failed send reported, in first-reference order.
    names: Vec<String>,
    /// The send these names failed under (S43c fix 3): what this panel's answers are
    /// checked against — captured at panel-build time, never re-read from the wire.
    failed_send: Option<FailedSend>,
}

impl SetSecretRow {
    /// The row for `names` — the `UnresolvedSecret` error's own missing names.
    pub fn new(id: impl Into<ElementId>, names: Vec<String>) -> Self {
        Self {
            id: id.into(),
            names,
            failed_send: None,
        }
    }

    /// Bind the row to the send its names failed with (S43c fix 3).
    pub fn with_failed_send(mut self, failed_send: Option<FailedSend>) -> Self {
        self.failed_send = failed_send;
        self
    }
}

impl RenderOnce for SetSecretRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let resend_ready = resend_ready(&self.id, &self.names, self.failed_send.as_ref(), cx);
        let note = pending_note(&self.id, &self.names, self.failed_send.as_ref(), cx);
        let base = self.id;
        v_flex()
            .w_full()
            .gap(DynamicSpacing::Base03.rems(cx))
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .gap(DynamicSpacing::Base03.rems(cx))
                    .children(self.names.iter().map(|name| {
                        // Display sanitized (`display_name` in the label and tooltip),
                        // identity and lookup untouched: the element id is the raw name,
                        // so two names differing only in stripped characters stay two
                        // buttons (S43c fix 2/3), and the click dispatches the raw one.
                        let display = display_name(name);
                        let selector = set_button_selector(&display);
                        let button_name = name.clone();
                        let row_names = self.names.clone();
                        let failed_send = self.failed_send.clone();
                        let panel = base.clone();
                        div().debug_selector(move || selector).child(
                            Button::new(child_id(&panel, format!("set-{name}")), set_label(name))
                                .size(ButtonSize::Compact)
                                .tooltip(Tooltip::text(set_tooltip(name)))
                                .on_click(move |_event, window, cx| {
                                    open_editor_for(
                                        &panel,
                                        &button_name,
                                        &row_names,
                                        failed_send.clone(),
                                        window,
                                        cx,
                                    )
                                }),
                        )
                    }))
                    .when(resend_ready, |this| {
                        this.child(
                            div()
                                .debug_selector(|| RESEND_BUTTON_SELECTOR.to_owned())
                                .child(
                                    Button::new(child_id(&base, "resend"), RESEND_LABEL)
                                        .size(ButtonSize::Compact)
                                        .tooltip(Tooltip::text("Send the request again"))
                                        .on_click(|_event, window, cx| resend(window, cx)),
                                ),
                        )
                    }),
            )
            .when_some(note, |this, note| {
                let selector = pending_note_selector(&note);
                this.child(
                    div()
                        .min_w_0()
                        .debug_selector(move || selector)
                        .child(Label::new(note).size(LabelSize::Small).color(Color::Muted)),
                )
            })
    }
}

/// Open the variables editor on `focus`'s secret row, asking the question `panel`'s
/// missing names pose (module docs). The editor is opened the one way this crate may
/// reach it: the `environment::EditSecret` action its own crate answers.
pub fn open_editor_for(
    panel: &ElementId,
    focus: &str,
    missing: &[String],
    failed_send: Option<FailedSend>,
    window: &mut Window,
    cx: &mut App,
) {
    let flow = flow(cx);
    flow.update(cx, |flow, cx| {
        flow.open_editor(panel, focus, missing, failed_send, window, cx)
    });
}

/// Re-send the request — what Resend offers once every missing name resolves. The Send
/// button's own action, dispatched so the request editor runs the same send it always
/// does (the `error_panel` "Open in Console" shape).
fn resend(window: &mut Window, cx: &mut App) {
    window.dispatch_action(SendRequest.boxed_clone(), cx);
}

/// The one flow, reached through an app-global (module docs). Created on the first Set
/// click — a render may only read it.
struct FlowHandle(Entity<SetSecretFlow>);

impl Global for FlowHandle {}

fn flow(cx: &mut App) -> Entity<SetSecretFlow> {
    if let Some(handle) = cx.try_global::<FlowHandle>() {
        return handle.0.clone();
    }
    let flow = cx.new(|_| SetSecretFlow::default());
    cx.set_global(FlowHandle(flow.clone()));
    flow
}

/// The question `panel` is asking (module docs): its missing names under the send its
/// panel failed with — never the send that happens to be installed now.
fn question(panel: &ElementId, names: &[String], failed_send: Option<&FailedSend>) -> Question {
    Question {
        panel: panel.clone(),
        names: names.to_vec(),
        send: failed_send.map(|failed| failed.id),
    }
}

/// One failed send, captured when its panel is built (S43c fix 3): the send the panel's
/// names could not resolve under, kept beside the identity its answers are keyed by and
/// the inputs its re-checks resolve against.
#[derive(Clone)]
pub struct FailedSend {
    /// A monotonic id, allocated per capture (module docs): an [`Arc`]'s address can be
    /// recycled after a drop — this identity never is.
    id: u64,
    /// The send's own inputs, exactly as the send ran them.
    pub send: Arc<SendInput>,
}

impl FailedSend {
    /// The next capture id (module docs).
    fn next_id() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
}

impl fmt::Debug for FailedSend {
    /// The capture id only: a request can carry values in its fields, and nothing that
    /// may be formatted into a message may hold one (AGENTS.md › Security).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FailedSend").field("id", &self.id).finish()
    }
}

/// Capture the send that just failed — the one the wire's inputs name at panel-build
/// time (module docs). [`None`] when no send is installed: such a panel answers only
/// "the send's resolution is not connected" (S43c fix 1, item 3).
pub fn capture_failed_send(cx: &App) -> Option<FailedSend> {
    let inputs = cx.try_global::<SetSecretInputs>()?;
    Some(FailedSend {
        id: FailedSend::next_id(),
        send: Arc::clone(&inputs.send),
    })
}

/// This panel's answer (module docs) — [`None`] until its own flow has one.
fn answer<'a>(
    panel: &ElementId,
    names: &[String],
    failed_send: Option<&FailedSend>,
    cx: &'a App,
) -> Option<&'a Option<SharedString>> {
    let flow = cx.try_global::<FlowHandle>()?;
    flow.0
        .read(cx)
        .answers
        .get(&question(panel, names, failed_send))
}

/// Whether every name this panel is about resolves now (module docs): the one condition
/// Resend is offered under.
fn resend_ready(
    panel: &ElementId,
    names: &[String],
    failed_send: Option<&FailedSend>,
    cx: &App,
) -> bool {
    matches!(answer(panel, names, failed_send, cx), Some(None))
}

/// Why this panel does not offer Resend right now (module docs), when its flow knows.
fn pending_note(
    panel: &ElementId,
    names: &[String],
    failed_send: Option<&FailedSend>,
    cx: &App,
) -> Option<SharedString> {
    answer(panel, names, failed_send, cx).and_then(|note| note.clone())
}

/// The failed send's own resolution inputs (module docs): the request, scopes, keychain
/// and key mapping [`prepare`] takes — what the re-check runs instead of guessing at
/// scopes. The app's wire installs them with [`install_inputs`].
#[derive(Clone)]
pub struct SetSecretInputs {
    /// The send's inputs, exactly as the send ran them — what [`capture_failed_send`]
    /// captures for the panel the failure is about.
    pub send: Arc<SendInput>,
}

impl Global for SetSecretInputs {}

/// Install the failed send's inputs: the next re-check resolves against them.
pub fn install_inputs(cx: &mut App, send: Arc<SendInput>) {
    cx.set_global(SetSecretInputs { send });
}

/// Why Resend is not offered when the re-check cannot run at all (module docs): a
/// re-check never returns silently.
pub const NO_RESOLUTION_NOTE: &str =
    "Resend is not offered: the send's resolution is not connected in this window yet.";

/// Why Resend is not offered when the send does not prepare cleanly (S43c fix 2): any
/// other preparation failure — a variable cycle, a URL that no longer parses — is a
/// different failure and re-sending would meet it again. The message names no input:
/// preparation errors can quote it.
pub const PREPARE_FAILED_NOTE: &str =
    "Resend is not offered: the request does not prepare cleanly — re-sending would fail again.";

/// Why Resend is not offered while names still resolve to nothing (module docs) — the
/// names as the labels show them ([`display_name`]), never their values.
pub fn missing_note(names: &[String]) -> String {
    let names = names
        .iter()
        .map(|name| format!("`{}`", display_name(name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Resend is not offered: the send still finds no value for {names}.")
}

/// The debug key a pending note renders under — derived from its rendered text, so
/// rewording it turns a stale test red instead of green (the `error_fix_selector` shape).
pub fn pending_note_selector(text: &str) -> String {
    format!("set-secret-note-{text}")
}

/// A variable name or a message as it may be shown (module docs): every Unicode
/// control, format, line- or paragraph-separator character stripped — bidi controls,
/// overrides and isolates, zero-width characters, U+2028/U+2029 — so crafted text cannot
/// reorder or hide what a label, headline, tooltip or note says. Display only — every
/// lookup keeps the text as it is (S43c fix 2).
pub fn display_name(text: &str) -> String {
    text.chars().filter(|c| !is_invisible(*c)).collect()
}

/// The invisible characters shown text must never carry (S43c fix 2/3): control
/// characters (Cc) plus the Unicode format characters relevant to spoofing — the bidi
/// marks, overrides and isolates, the Mongolian selectors, the zero-width characters,
/// the line and paragraph separators, the interlinear-annotation characters and the tag
/// block.
fn is_invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'..='\u{180F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{E0000}'..='\u{E007F}'
        )
}

/// The one question an answer belongs to (S43c fix 2): the panel asking — its element
/// id, one per response region and hashed by value, so two response regions never share
/// a key — the names it asks about, and the send it failed with, by identity (its
/// [`Arc`]'s address: re-installing the same send keeps the same question, a different
/// send is a different one). Two panels failing on the same name in
/// different scopes ask different questions: they never share an answer, and answering
/// one never withdraws another's.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Question {
    panel: ElementId,
    names: Vec<String>,
    /// The captured failed send's identity — see [`FailedSend::id`].
    send: Option<u64>,
}

/// The flow's state (module docs): every question's answer, the question the open editor
/// is answering, and the two pieces of async work whose cancellation would lose the flow.
#[derive(Default)]
struct SetSecretFlow {
    /// Each question's answer: [`None`] = every name resolves and Resend is offered,
    /// otherwise the note saying why it is not (module docs). Keys are per panel — one
    /// panel's answer is never reset by another's (S43c fix 2).
    answers: HashMap<Question, Option<SharedString>>,
    /// The question the open editor is answering, with the send its names must resolve
    /// against — captured with the question, so a later install changes neither
    /// (module docs).
    active: Option<(Question, Option<Arc<SendInput>>)>,
    /// A modal is on screen: the editor this flow's click asked for (module docs).
    editor_open: bool,
    /// The workspace observation that spots the editor going (module docs).
    watch: Option<Subscription>,
    /// The resolution re-check in flight; replacing it cancels the old (AGENTS.md ›
    /// Async).
    recheck: Option<Task<()>>,
}

impl SetSecretFlow {
    /// Ask for the editor on `focus`'s row with `panel`'s question attached (module
    /// docs), then run the re-check that answers it.
    fn open_editor(
        &mut self,
        panel: &ElementId,
        focus: &str,
        missing: &[String],
        failed_send: Option<FailedSend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Some(workspace)) = window.root::<Workspace>() else {
            // The panel only draws under a workspace shell. A bare window has no editor
            // to open and no toast layer to say so (`save_body`'s documented shape).
            return;
        };
        // The question and the send it resolves against are captured together (module
        // docs): a later install changes neither this answer's key nor its world, and
        // another panel's question never lands here.
        let question = question(panel, missing, failed_send.as_ref());
        let send = failed_send.map(|failed| failed.send);
        self.active = Some((question, send));
        self.editor_open = workspace.read(cx).modal_layer().is_open();
        self.watch = Some(cx.observe(&workspace, Self::on_workspace_changed));
        window.dispatch_action(
            EditSecret {
                name: focus.to_string(),
            }
            .boxed_clone(),
            cx,
        );
        self.recheck(cx);
    }

    /// The workspace changed: the editor this flow's click asked for either landed or
    /// went — and going is when the resolution is worth running again (module docs).
    /// `is_open` is all the modal layer says to a crate that may not name the editor.
    fn on_workspace_changed(&mut self, workspace: Entity<Workspace>, cx: &mut Context<Self>) {
        let open = workspace.read(cx).modal_layer().is_open();
        if open {
            self.editor_open = true;
        } else if self.editor_open {
            self.editor_open = false;
            self.recheck(cx);
        }
    }

    /// Run the send's own resolution again — [`prepare`] over the question's own send
    /// (module docs) — off the foreground, and record what it says under that question's
    /// key alone.
    fn recheck(&mut self, cx: &mut Context<Self>) {
        let Some((question, send)) = self.active.clone() else {
            return;
        };
        let Some(send) = send else {
            self.answers
                .insert(question, Some(SharedString::new_static(NO_RESOLUTION_NOTE)));
            // The panel reads this flow at render and the view tree drawing it is not
            // this entity's to notify (module docs): the app-wide refresh is the signal
            // that re-renders it.
            cx.refresh_windows();
            return;
        };
        self.recheck = Some(cx.spawn(async move |this, cx| {
            let note = cx
                .background_spawn(async move { resolution_note(&send) })
                .await;
            if this
                .update(cx, |this, cx| {
                    this.answers.insert(question, note);
                    // See the early return above for why this is app-wide.
                    cx.refresh_windows();
                })
                .is_err()
            {
                // Handled: the flow went away with its window; there is no panel left to
                // offer anything on.
            }
        }));
    }
}

/// What the send's own preparation says about Resend (module docs): [`None`] only when
/// [`prepare`] succeeds — every name resolves and re-sending goes out. An
/// `UnresolvedSecret` names the ones that do not; any other preparation failure — a
/// variable cycle, a URL that no longer parses — is a different failure that re-sending
/// would meet again, so it never offers Resend either (S43c fix 2). Preparation errors
/// can quote input, so only the two static shapes above are ever shown. Runs off the
/// foreground.
fn resolution_note(send: &SendInput) -> Option<SharedString> {
    let ancestors: Vec<&Auth> = send.ancestors.iter().collect();
    let mut dynamic = DynamicValues::new();
    match prepare(
        &send.request,
        &ancestors,
        &send.scopes,
        send.secrets.as_ref(),
        send.secret_key.as_ref(),
        &mut dynamic,
    ) {
        Ok(_) => None,
        Err(PipelineError::Transport(TransportError::UnresolvedSecret { names })) => {
            Some(SharedString::from(missing_note(&names)))
        }
        Err(_) => Some(SharedString::new_static(PREPARE_FAILED_NOTE)),
    }
}

#[cfg(test)]
#[path = "set_secret_tests.rs"]
mod tests;
