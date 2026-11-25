//! The Message log toolbar (REQ-LOG-03): a search field, direction and type filter
//! chips, Pause / Resume of the log's auto-scroll, Clear, Copy (the selected row's
//! payload) and Export (the session as JSON lines).
//!
//! Every control is an action in the `message_log` namespace, so the command palette
//! lists it while the log has focus, and every control is a tab stop with a label. The
//! handlers live on [`SessionLog`] (the `save_body` shape: the view that owns the state
//! answers its actions); `session_log.rs` only mounts the toolbar and registers them.
//!
//! The search and the chips write one [`SessionFilter`] into the [`SessionStore`], which
//! computes the matching rows once ([`SessionStore::set_filter`]); the log lists what the
//! store says. A filter the store rejects is shown inline under the toolbar.
//!
//! `Decision:` the search field is created on the log's first render, because a
//! [`TextInput`] needs a window and [`SessionLog::new`] has none (its signature is the
//! S51c contract its tests hold).
//!
//! `Decision:` Clear asks first, inline in the toolbar ("Clear all 12 messages?" with
//! Clear and Cancel), because a cleared session cannot be brought back. `ConfirmClear`
//! and `CancelClear` are the two answers, as actions too.
//!
//! `Decision:` Pause stops only the follow-to-bottom: events keep arriving and the list
//! keeps growing under the rows on screen. Resume jumps to the newest row.
//!
//! `Decision:` Copy puts a text payload on the clipboard as text and a binary one (or
//! text that is not UTF-8) as base64, the export's encoding. A spilled payload is read
//! off the foreground, up to [`FULL_PAYLOAD_CAP`]; a bigger one points at Export.
//!
//! `Decision:` Export takes a [`SessionStore::snapshot`] once the file is chosen and
//! writes it on a background task with the store's own JSONL writer. The snapshot shares
//! the spill files, so a Clear during the export deletes nothing it still reads.
//!
//! Clear drops the spill files [`SessionStore::clear`] hands back on a background task:
//! deleting files is IO (#341).

use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use api_model::{Payload, SessionEvent, SessionMessageKind};
use base64::Engine as _;
use gpui::{
    AnyElement, ClipboardItem, Context, Div, Entity, Focusable as _, Stateful, Subscription, Task,
    Window,
};
use pustman_actions::message_log::{
    CancelClear, Clear, ClearFilters, ConfirmClear, CopyPayload, Export, FilterBinary,
    FilterEvents, FilterJson, FilterReceived, FilterSent, FilterText, FocusSearch, Pause, Resume,
};
use text_input::{TextInput, TextInputEvent};
use ui::prelude::*;

use crate::child_id;
use crate::save_body::show_toast;
use crate::session_log::{FULL_PAYLOAD_CAP, SessionLog, read_spilled};
use crate::session_store::{SessionDirection, SessionFilter, SessionSnapshot};

/// The name the export's save dialog suggests.
const SUGGESTED_EXPORT_NAME: &str = "session.jsonl";

/// The toolbar's state, held by the [`SessionLog`].
#[derive(Default)]
pub(crate) struct ToolbarState {
    search: Option<Entity<TextInput>>,
    filter: SessionFilter,
    filter_error: Option<SharedString>,
    paused: bool,
    confirming_clear: bool,
    /// The in-flight export and copy: a second one replaces (cancels) the first.
    export_task: Option<Task<()>>,
    copy_task: Option<Task<()>>,
    _search_subscription: Option<Subscription>,
}

impl ToolbarState {
    pub(crate) fn paused(&self) -> bool {
        self.paused
    }
}

impl SessionLog {
    /// The toolbar's search field, once the log has been drawn.
    pub fn search_input(&self) -> Option<&Entity<TextInput>> {
        self.toolbar.search.as_ref()
    }

    /// Whether the log has stopped following new rows.
    pub fn is_paused(&self) -> bool {
        self.toolbar.paused
    }

    /// Whether Clear is waiting for its confirmation.
    pub fn is_confirming_clear(&self) -> bool {
        self.toolbar.confirming_clear
    }

    /// Create the search field on the first render (see the module docs).
    pub(crate) fn ensure_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toolbar.search.is_some() {
            return;
        }
        let search = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(SharedString::new_static("Search messages"))
        });
        search.read(cx).focus_handle(cx).tab_stop(true);
        self.toolbar._search_subscription = Some(cx.subscribe(
            &search,
            |log, search, event: &TextInputEvent, cx| {
                if matches!(event, TextInputEvent::Changed) {
                    let text = search.read(cx).text();
                    log.update_filter(|filter| filter.text = text, cx);
                }
            },
        ));
        self.toolbar.search = Some(search);
    }

    /// Register every toolbar action on the log's root element.
    pub(crate) fn register_toolbar_actions(
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(|log, _: &FilterSent, _, cx| {
                log.toggle_direction(SessionDirection::Sent, cx)
            }))
            .on_action(cx.listener(|log, _: &FilterReceived, _, cx| {
                log.toggle_direction(SessionDirection::Received, cx)
            }))
            .on_action(cx.listener(|log, _: &FilterText, _, cx| {
                log.toggle_kind(SessionMessageKind::Text, cx)
            }))
            .on_action(cx.listener(|log, _: &FilterJson, _, cx| {
                log.toggle_kind(SessionMessageKind::Json, cx)
            }))
            .on_action(cx.listener(|log, _: &FilterBinary, _, cx| {
                log.toggle_kind(SessionMessageKind::Binary, cx)
            }))
            .on_action(cx.listener(|log, _: &FilterEvents, _, cx| {
                log.toggle_kind(SessionMessageKind::Event, cx)
            }))
            .on_action(cx.listener(Self::clear_filters))
            .on_action(cx.listener(Self::pause))
            .on_action(cx.listener(Self::resume))
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(Self::confirm_clear))
            .on_action(cx.listener(Self::cancel_clear))
            .on_action(cx.listener(Self::copy_payload))
            .on_action(cx.listener(Self::export))
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = &self.toolbar.search {
            let handle = search.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    fn toggle_direction(&mut self, direction: SessionDirection, cx: &mut Context<Self>) {
        self.update_filter(
            |filter| {
                filter.direction = (filter.direction != Some(direction)).then_some(direction);
            },
            cx,
        );
    }

    fn toggle_kind(&mut self, kind: SessionMessageKind, cx: &mut Context<Self>) {
        self.update_filter(
            |filter| filter.kind = (filter.kind != Some(kind)).then_some(kind),
            cx,
        );
    }

    fn clear_filters(&mut self, _: &ClearFilters, _window: &mut Window, cx: &mut Context<Self>) {
        self.update_filter(|filter| *filter = SessionFilter::default(), cx);
        if let Some(search) = self.toolbar.search.clone()
            && !search.read(cx).text().is_empty()
        {
            // The field's own `Changed` re-applies the (now empty) text.
            search.update(cx, |search, cx| search.set_text("", cx));
        }
    }

    /// Change the filter and hand it to the store, which recomputes the listed rows.
    fn update_filter(&mut self, change: impl FnOnce(&mut SessionFilter), cx: &mut Context<Self>) {
        change(&mut self.toolbar.filter);
        let filter = self.toolbar.filter.clone();
        let applied = self.store.update(cx, |store, cx| {
            let applied = store.set_filter(filter);
            cx.notify();
            applied
        });
        self.toolbar.filter_error = applied
            .err()
            .map(|error| format!("The search could not be applied: {error}").into());
        cx.notify();
    }

    fn pause(&mut self, _: &Pause, _window: &mut Window, cx: &mut Context<Self>) {
        self.toolbar.paused = true;
        cx.notify();
    }

    fn resume(&mut self, _: &Resume, _window: &mut Window, cx: &mut Context<Self>) {
        self.toolbar.paused = false;
        self.scroll_to_end(cx);
        cx.notify();
    }

    fn clear(&mut self, _: &Clear, _window: &mut Window, cx: &mut Context<Self>) {
        if self.store.read(cx).is_empty() {
            return;
        }
        self.toolbar.confirming_clear = true;
        cx.notify();
    }

    fn cancel_clear(&mut self, _: &CancelClear, _window: &mut Window, cx: &mut Context<Self>) {
        self.toolbar.confirming_clear = false;
        cx.notify();
    }

    fn confirm_clear(&mut self, _: &ConfirmClear, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.toolbar.confirming_clear {
            return;
        }
        self.toolbar.confirming_clear = false;
        let spill_files = self.store.update(cx, |store, cx| {
            let spill_files = store.clear();
            cx.notify();
            spill_files
        });
        if !spill_files.is_empty() {
            // Dropping the handles deletes the files; nothing awaits the deletion.
            cx.background_spawn(async move { drop(spill_files) })
                .detach();
        }
        cx.notify();
    }

    fn copy_payload(&mut self, _: &CopyPayload, window: &mut Window, cx: &mut Context<Self>) {
        let window_handle = window.window_handle();
        let Some(seq) = self.selected else {
            show_toast(
                window_handle,
                "Select a message to copy its payload.",
                Severity::Warning,
                cx,
            );
            return;
        };
        let message = match self.store.read(cx).events().get(seq) {
            Some(SessionEvent::Sent(message) | SessionEvent::Received(message)) => message,
            _ => {
                show_toast(
                    window_handle,
                    "This event has no payload to copy.",
                    Severity::Warning,
                    cx,
                );
                return;
            }
        };
        let kind = message.kind;
        match &message.payload {
            Payload::Inline(bytes) => {
                cx.write_to_clipboard(ClipboardItem::new_string(clipboard_text(kind, bytes)));
            }
            Payload::Spilled { len, .. } if *len > FULL_PAYLOAD_CAP => {
                show_toast(
                    window_handle,
                    "This payload is too large to copy. Export the session to get it whole.",
                    Severity::Warning,
                    cx,
                );
            }
            Payload::Spilled { path, len, prefix } => {
                let path = path.clone();
                let prefix = prefix.clone();
                let len = *len;
                self.toolbar.copy_task = Some(cx.spawn(async move |_this, cx| {
                    let copied = cx
                        .background_spawn(async move {
                            read_spilled(&path, &prefix, len)
                                .map(|bytes| clipboard_text(kind, &bytes))
                        })
                        .await;
                    cx.update(|cx| match copied {
                        Ok(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                        Err(error) => show_toast(
                            window_handle,
                            format!("Could not copy the payload — {error}"),
                            Severity::Error,
                            cx,
                        ),
                    });
                }));
            }
        }
    }

    fn export(&mut self, _: &Export, window: &mut Window, cx: &mut Context<Self>) {
        let window_handle = window.window_handle();
        if self.store.read(cx).is_empty() {
            show_toast(
                window_handle,
                "There are no messages to export yet.",
                Severity::Warning,
                cx,
            );
            return;
        }
        let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let chosen = cx.prompt_for_new_path(&directory, Some(SUGGESTED_EXPORT_NAME));
        self.toolbar.export_task = Some(cx.spawn(async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return,
                Ok(Err(error)) => {
                    cx.update(|cx| {
                        show_toast(
                            window_handle,
                            format!("Could not open the save dialog — {error}"),
                            Severity::Error,
                            cx,
                        );
                    });
                    return;
                }
                Err(_canceled) => {
                    cx.update(|cx| {
                        show_toast(
                            window_handle,
                            "The save dialog did not return a file name",
                            Severity::Error,
                            cx,
                        );
                    });
                    return;
                }
            };
            let Ok(snapshot) = this.read_with(cx, |log, cx| log.store.read(cx).snapshot()) else {
                // Handled: the log went away with its window; there is no session left.
                return;
            };
            let written = cx
                .background_spawn(async move { write_session(&path, &snapshot) })
                .await;
            if let Err(error) = written {
                cx.update(|cx| {
                    show_toast(
                        window_handle,
                        format!("Could not export the session — {error:#}"),
                        Severity::Error,
                        cx,
                    );
                });
            }
        }));
    }

    /// The toolbar row, plus the clear confirmation or the filter error under it.
    pub(crate) fn render_toolbar(&self, cx: &Context<Self>) -> AnyElement {
        let base = &self.base;
        let weak = cx.entity().downgrade();
        let filter = &self.toolbar.filter;

        let chip = |name: &'static str,
                    label: &'static str,
                    on: bool,
                    apply: fn(&mut SessionLog, &mut Context<SessionLog>)| {
            let weak = weak.clone();
            Toggle::new(
                child_id(base, name),
                SharedString::new_static(label),
                ToggleState::from(on),
            )
            .on_change(move |_state, _window, cx| {
                if weak.update(cx, apply).is_err() {
                    // Handled: the log went away with its window.
                }
            })
        };
        let button =
            |name: &'static str,
             label: &'static str,
             icon: IconName,
             tooltip: &'static str,
             apply: fn(&mut SessionLog, &mut Window, &mut Context<SessionLog>)| {
                let weak = weak.clone();
                Button::new(child_id(base, name), label)
                    .size(ButtonSize::Compact)
                    .start_icon(Icon::new(icon).size(IconSize::Small))
                    .tooltip(Tooltip::text(tooltip))
                    .on_click(move |_event, window, cx| {
                        if weak.update(cx, |log, cx| apply(log, window, cx)).is_err() {
                            // Handled: the log went away with its window.
                        }
                    })
            };

        let search = self.toolbar.search.clone().map(|search| {
            div()
                .flex_1()
                .min_w_0()
                .px(DynamicSpacing::Base03.rems(cx))
                .border_1()
                .border_color(cx.theme().colors().border)
                .rounded_sm()
                .child(search)
        });
        let pause = if self.toolbar.paused {
            button(
                "resume",
                "Resume",
                IconName::Play,
                "Follow new messages to the bottom",
                |log, window, cx| log.resume(&Resume, window, cx),
            )
        } else {
            button(
                "pause",
                "Pause",
                IconName::Pause,
                "Stop following new messages; they keep arriving",
                |log, window, cx| log.pause(&Pause, window, cx),
            )
        };

        let row = h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap(DynamicSpacing::Base03.rems(cx))
            .children(search)
            .child(chip(
                "filter-sent",
                "Sent",
                filter.direction == Some(SessionDirection::Sent),
                |log, cx| log.toggle_direction(SessionDirection::Sent, cx),
            ))
            .child(chip(
                "filter-received",
                "Received",
                filter.direction == Some(SessionDirection::Received),
                |log, cx| log.toggle_direction(SessionDirection::Received, cx),
            ))
            .child(chip(
                "filter-text",
                "Text",
                filter.kind == Some(SessionMessageKind::Text),
                |log, cx| log.toggle_kind(SessionMessageKind::Text, cx),
            ))
            .child(chip(
                "filter-json",
                "JSON",
                filter.kind == Some(SessionMessageKind::Json),
                |log, cx| log.toggle_kind(SessionMessageKind::Json, cx),
            ))
            .child(chip(
                "filter-binary",
                "Binary",
                filter.kind == Some(SessionMessageKind::Binary),
                |log, cx| log.toggle_kind(SessionMessageKind::Binary, cx),
            ))
            .child(chip(
                "filter-events",
                "Events",
                filter.kind == Some(SessionMessageKind::Event),
                |log, cx| log.toggle_kind(SessionMessageKind::Event, cx),
            ))
            .child(pause)
            .child(button(
                "copy",
                "Copy",
                IconName::Copy,
                "Copy the selected message's payload",
                |log, window, cx| log.copy_payload(&CopyPayload, window, cx),
            ))
            .child(button(
                "export",
                "Export",
                IconName::Download,
                "Export the session as JSON lines",
                |log, window, cx| log.export(&Export, window, cx),
            ))
            .child(button(
                "clear",
                "Clear",
                IconName::Trash,
                "Remove every message from the log",
                |log, window, cx| log.clear(&Clear, window, cx),
            ));

        let confirm = self.toolbar.confirming_clear.then(|| {
            let count = self.store.read(cx).len();
            h_flex()
                .w_full()
                .items_center()
                .gap(DynamicSpacing::Base03.rems(cx))
                .child(
                    Label::new(format!(
                        "Clear all {count} messages? This cannot be undone."
                    ))
                    .size(LabelSize::Small),
                )
                .child(
                    button(
                        "confirm-clear",
                        "Clear",
                        IconName::Trash,
                        "Remove every message now",
                        |log, window, cx| log.confirm_clear(&ConfirmClear, window, cx),
                    )
                    .style(ButtonStyle::Tinted(TintColor::Error)),
                )
                .child(button(
                    "cancel-clear",
                    "Cancel",
                    IconName::X,
                    "Keep the messages",
                    |log, window, cx| log.cancel_clear(&CancelClear, window, cx),
                ))
        });
        let error = self
            .toolbar
            .filter_error
            .clone()
            .map(|error| Label::new(error).size(LabelSize::Small).color(Color::Error));

        v_flex()
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(row)
            .children(confirm)
            .children(error)
            .into_any_element()
    }
}

/// A payload as clipboard text: text as it is, anything else as base64.
fn clipboard_text(kind: SessionMessageKind, bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if kind != SessionMessageKind::Binary => text.to_owned(),
        _ => base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

/// Write `snapshot` to `path` as JSON lines (see the module docs). Blocking IO: run it on
/// a background task.
fn write_session(path: &Path, snapshot: &SessionSnapshot) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    snapshot
        .export_jsonl(&mut BufWriter::new(file))
        .with_context(|| format!("could not write {}", path.display()))
}

#[cfg(test)]
#[path = "log_toolbar_tests.rs"]
mod tests;
