//! The resizable vertical split the response view is embedded in (S29e): the request
//! editor's own content on top, the [`response_viewer::ResponseView`] a send's result
//! lands in below, and a drag handle between them that moves the boundary.
//!
//! `Decision:` the split fraction lives in [`RequestEditor`]'s state and is not
//! persisted (the step's scope): it is a per-tab layout preference of this session, and
//! persisting it would need a workspace-format change this step does not make.

use gpui::{DragMoveEvent, Empty, EntityId, KeyDownEvent, Role, relative};
use ui::prelude::*;

use crate::RequestEditor;

/// Test hook: the drag handle between the request and response panes.
pub(crate) const SPLIT_HANDLE_SELECTOR: &str = "response-split-handle";

/// Test hook: the 1 px line drawn inside the handle.
pub(crate) const SPLIT_LINE_SELECTOR: &str = "response-split-line";

/// The share of the split the request pane opens at.
pub(crate) const DEFAULT_SPLIT_FRACTION: f32 = 0.5;

/// The smallest share either pane may take: clamping into this range is what keeps a
/// drag or a key press from collapsing one pane below a usable minimum.
const MIN_SPLIT_FRACTION: f32 = 0.2;
const MAX_SPLIT_FRACTION: f32 = 0.8;

/// How far one Up or Down press moves the handle: a nudge, not a jump.
const KEYBOARD_STEP: f32 = 0.05;

/// The drag the handle starts. It names the owning editor, so the move listeners on
/// other editors' splits ignore a drag that is not theirs.
#[derive(Debug)]
struct SplitDrag {
    editor: EntityId,
}

impl RequestEditor {
    /// The editor's body in one resizable split: `request_content` above, the embedded
    /// [`response_viewer::ResponseView`] below, the handle between them.
    pub(super) fn render_response_split(
        &self,
        request_content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .size_full()
            .min_h_0()
            // The move listener is on the split itself, so the drag's bounds are the
            // split's: the fraction is where the mouse sits inside it.
            .on_drag_move::<SplitDrag>(cx.listener(Self::handle_split_drag))
            .child(
                v_flex()
                    .h(relative(self.split_fraction))
                    .flex_shrink_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(request_content),
            )
            .child(self.render_split_handle(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.response_view.clone()),
            )
            .into_any_element()
    }

    /// The handle between the panes: a tab stop — Up and Down resize while it has focus
    /// — that also drags with the mouse.
    fn render_split_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors();
        div()
            .id(ElementId::from((
                ElementId::from(&self.focus_handle),
                SPLIT_HANDLE_SELECTOR,
            )))
            .debug_selector(|| SPLIT_HANDLE_SELECTOR.to_owned())
            .track_focus(&self.split_focus)
            .group(SPLIT_HANDLE_SELECTOR)
            // A transparent strip to grab, a 1 px line to see (#224): the pane edges
            // stay calm while the hit area stays wide enough for a mouse.
            .h(DynamicSpacing::Base08.rems(cx))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .cursor(CursorStyle::ResizeUpDown)
            .focus_visible(|style| style.bg(colors.border_focused))
            .child(
                div()
                    .debug_selector(|| SPLIT_LINE_SELECTOR.to_owned())
                    .h(px_to_rems(1.))
                    .w_full()
                    .bg(colors.border)
                    .group_hover(SPLIT_HANDLE_SELECTOR, |style| {
                        style.bg(colors.border_focused)
                    }),
            )
            .role(Role::Splitter)
            .aria_label(SharedString::new_static(
                "Resize the request and response panes",
            ))
            .tooltip(
                Tooltip::text(SharedString::new_static(
                    "Drag to resize — or press Up and Down while this handle has focus",
                ))
                .builder(),
            )
            .on_drag(
                SplitDrag {
                    editor: cx.entity_id(),
                },
                |_, _, _, cx| cx.new(|_| Empty),
            )
            .on_key_down(cx.listener(Self::handle_split_key))
            .into_any_element()
    }

    /// The handle was dragged: the request pane takes the share of the split the mouse
    /// sits at, clamped so neither pane collapses. The drag's payload names its editor,
    /// so a drag on this split never resizes another editor's.
    fn handle_split_drag(
        &mut self,
        event: &DragMoveEvent<SplitDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.drag(cx).editor != cx.entity_id() {
            return;
        }
        cx.set_active_drag_cursor_style(CursorStyle::ResizeUpDown, window);
        let height = event.bounds.size.height;
        if height <= px(0.) {
            return;
        }
        self.set_split_fraction(
            (event.event.position.y - event.bounds.origin.y) / height,
            cx,
        );
    }

    /// Up and Down move the handle one step while it has focus — the keyboard's side of
    /// the drag. Any other key is left alone.
    fn handle_split_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = match event.keystroke.key.as_str() {
            "up" => -KEYBOARD_STEP,
            "down" => KEYBOARD_STEP,
            _ => return,
        };
        self.set_split_fraction(self.split_fraction + step, cx);
        cx.stop_propagation();
    }

    /// The one place the split fraction is written, so every path clamps it the same
    /// way and redraws only when the fraction really moved.
    fn set_split_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let fraction = fraction.clamp(MIN_SPLIT_FRACTION, MAX_SPLIT_FRACTION);
        if (fraction - self.split_fraction).abs() > f32::EPSILON {
            self.split_fraction = fraction;
            cx.notify();
        }
    }
}

#[cfg(test)]
#[path = "response_split_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "response_split_divider_tests.rs"]
mod divider_tests;
