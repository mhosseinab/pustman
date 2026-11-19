//! The layers of the window and how a surface tells what layer it is on.
//!
//! Shadows are never stacked by hand: a control that floats asks for the elevation that
//! matches its layer ([`StyledExt::elevation_2`] for menus and tooltips,
//! [`StyledExt::elevation_3`] for dialogs) and gets that layer's background, border and
//! shadow as one decision (`references/design-system.md` §5).

use gpui::{App, Hsla, Styled};
use theme::ActiveTheme;

use crate::units::px_to_rems;

/// The radius of a dialog surface: the redesign pack's `radius.dialog`, 10 px.
const DIALOG_RADIUS_PX: f32 = 10.;

/// How far a surface sits above the window background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElevationIndex {
    /// The window itself.
    Background,
    /// The main content sheet.
    Surface,
    /// Where code is read: request and response editors.
    EditorSurface,
    /// Menus, popovers, pickers, tooltips.
    ElevatedSurface,
    /// Modal dialogs.
    ModalSurface,
}

impl ElevationIndex {
    /// The background of this layer.
    pub fn bg(self, cx: &App) -> Hsla {
        let colors = cx.theme().colors();
        match self {
            Self::Background => colors.background,
            Self::Surface => colors.surface_background,
            Self::EditorSurface => colors.editor_background,
            Self::ElevatedSurface => colors.elevated_surface_background,
            Self::ModalSurface => colors.elevated_surface_background,
        }
    }

    /// The background content sitting on this layer draws against.
    ///
    /// Today that is the layer's own background — the theme has one token per layer, so
    /// the pair exists to keep the *question* ("what do I sit on?") distinct from the
    /// layer's own paint at the call site.
    pub fn on_elevation_bg(self, cx: &App) -> Hsla {
        self.bg(cx)
    }
}

/// Elevation helpers on top of [`Styled`].
pub trait StyledExt: Styled + Sized {
    /// A surface that floats slightly: dropdown triggers, raised cards.
    fn elevation_1(self, cx: &App) -> Self {
        self.bg(ElevationIndex::Surface.bg(cx)).shadow_xs()
    }

    /// Popovers, menus, pickers and tooltips: the elevated surface with its border and
    /// shadow.
    fn elevation_2(self, cx: &App) -> Self {
        self.bg(ElevationIndex::ElevatedSurface.bg(cx))
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .shadow_md()
    }

    /// Modal dialogs: the top layer, rounded like the pack's dialog radius.
    fn elevation_3(self, cx: &App) -> Self {
        self.bg(ElevationIndex::ModalSurface.bg(cx))
            .rounded(px_to_rems(DIALOG_RADIUS_PX))
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .shadow_xl()
    }
}

impl<E: Styled> StyledExt for E {}
