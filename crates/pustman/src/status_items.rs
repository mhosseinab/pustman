//! The status bar's readouts (PRD §3): the console count at the left and the active
//! environment at the right — the workbench blueprint's status-bar content (`Console  0`
//! in `docs/postman_redesign_pack/06_visuals/workbench_blueprint.svg`) — wired into every
//! [`Workspace`] window's status bar here in the app shell, the one crate that can see
//! both globals they read: `console`'s [`ConsoleStore`] and `environment_picker`'s
//! [`SelectorInputs`].

use console::ConsoleStore;
use environment_picker::SelectorInputs;
use environment_picker::selector::NO_ENVIRONMENT_LABEL;
use gpui::{App, Context, EntityId, Global, Subscription, WeakEntity};
use ui::prelude::*;
use workspace::Workspace;

/// The debug selector on the console-count readout's rendered element — the one string
/// tests hand to `VisualTestContext::debug_bounds`.
pub(crate) const CONSOLE_COUNT_SELECTOR: &str = "status-console-count";

/// The debug selector on the active-environment readout's rendered element.
pub(crate) const ENVIRONMENT_SELECTOR: &str = "status-environment";

/// The console entry count at the status bar's left: the blueprint's `Console  0`
/// readout (title, count). It follows [`ConsoleStore`] as entries arrive, are cleared,
/// or recording is paused.
pub(crate) struct ConsoleCount {
    count: usize,
    _store_observer: Subscription,
}

impl ConsoleCount {
    fn new(cx: &mut Context<Self>) -> Self {
        let count = cx.global::<ConsoleStore>().len();
        Self {
            count,
            _store_observer: cx.observe_global::<ConsoleStore>(|this, cx| {
                this.count = cx.global::<ConsoleStore>().len();
                cx.notify();
            }),
        }
    }

    /// The text the strip shows: the blueprint's `Console  0` (title, count).
    pub(crate) fn readout(&self) -> String {
        format!("Console  {}", self.count)
    }
}

impl Render for ConsoleCount {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .debug_selector(|| CONSOLE_COUNT_SELECTOR.to_owned())
            .child(
                Label::new(self.readout())
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
    }
}

/// The active environment at the status bar's right: its name, or the picker's
/// "No environment" label while none is active (pack screen S07's "No active environment"
/// state — nothing is fabricated). It follows [`SelectorInputs`]: the app's wire installs
/// them, and the picker writes a confirmed choice back.
pub(crate) struct ActiveEnvironment {
    name: Option<SharedString>,
    _inputs_observer: Subscription,
}

impl ActiveEnvironment {
    fn new(cx: &mut Context<Self>) -> Self {
        let name = active_environment_name(cx);
        Self {
            name,
            _inputs_observer: cx.observe_global::<SelectorInputs>(|this, cx| {
                this.name = active_environment_name(cx);
                cx.notify();
            }),
        }
    }

    /// The text the strip shows: the active environment's name, or "No environment".
    pub(crate) fn readout(&self) -> SharedString {
        self.name
            .clone()
            .unwrap_or_else(|| SharedString::new_static(NO_ENVIRONMENT_LABEL))
    }
}

impl Render for ActiveEnvironment {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let readout = self.readout();
        div()
            .id(ENVIRONMENT_SELECTOR)
            .debug_selector(|| ENVIRONMENT_SELECTOR.to_owned())
            .min_w_0()
            .tooltip(Tooltip::text(readout.clone()).builder())
            .child(
                Label::new(readout)
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .truncate(),
            )
    }
}

/// The installed inputs' active environment's name — `None` while no environment is
/// active, or before the wire installed the inputs (nothing is fabricated).
fn active_environment_name(cx: &App) -> Option<SharedString> {
    let inputs = cx.try_global::<SelectorInputs>()?;
    let active = inputs.active.as_ref()?;
    inputs
        .environments
        .iter()
        .find(|environment| &environment.id == active)
        .map(|environment| SharedString::from(environment.name.clone()))
}

/// The windows that already got their readouts, plus the subscription that wires the
/// next one. The subscription lives here for the app's lifetime — dropping it stops the
/// wiring (AGENTS.md).
#[derive(Default)]
struct StatusItemsWiring {
    started: bool,
    registered: Vec<EntityId>,
    subscriptions: Vec<Subscription>,
}

impl Global for StatusItemsWiring {}

/// Wire the readouts into every workspace window's status bar: the console count on the
/// left, the active environment on the right.
pub(crate) fn init(cx: &mut App) {
    if cx.default_global::<StatusItemsWiring>().started {
        return;
    }
    cx.default_global::<StatusItemsWiring>().started = true;
    let subscription = cx.observe_new::<Workspace>(|_workspace, _window, cx| {
        let workspace = cx.entity().downgrade();
        // Deferred: the window is on the update stack while its workspace is being
        // created (the shape the history-panel wire uses).
        cx.defer(move |cx| add_status_items(&workspace, cx));
    });
    cx.default_global::<StatusItemsWiring>()
        .subscriptions
        .push(subscription);
}

/// Add the readouts to one workspace's status bar, at most once per workspace whichever
/// path gets there first.
fn add_status_items(workspace: &WeakEntity<Workspace>, cx: &mut App) {
    let Some(workspace) = workspace.upgrade() else {
        // Gone with its window before the deferred wire ran.
        return;
    };
    let workspace_id = workspace.entity_id();
    if cx
        .default_global::<StatusItemsWiring>()
        .registered
        .contains(&workspace_id)
    {
        return;
    }
    let console_count = cx.new(ConsoleCount::new);
    let environment = cx.new(ActiveEnvironment::new);
    workspace.update(cx, |workspace, cx| {
        let status_bar = workspace.status_bar();
        status_bar.update(cx, |status_bar, cx| {
            status_bar.add_left_item(console_count, cx);
            status_bar.add_right_item(environment, cx);
        });
    });
    cx.default_global::<StatusItemsWiring>()
        .registered
        .push(workspace_id);
}

#[cfg(test)]
#[path = "status_items_tests.rs"]
mod tests;
