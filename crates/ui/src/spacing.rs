//! Spacing that follows the UI density setting.
//!
//! Feature code never writes `px(4.)` for an inner gap: it asks for
//! [`DynamicSpacing::Base04`], which resolves to a different pixel count per density
//! (`references/design-system.md` §4). Fixed geometry that must not move with density —
//! icon optical alignment, borders — uses the Tailwind spacing methods instead.

use gpui::{App, Global, Pixels, Rems, px};

use crate::units::px_to_rems;

/// How tightly the UI is packed. The setting lands in `settings` later; until then
/// [`set_ui_density`] picks one at runtime and [`Default`] is what a fresh app draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiDensity {
    /// Rows and gaps at 75% of the default size.
    Compact,
    /// The size the design system's px values are written for.
    #[default]
    Default,
    /// Rows and gaps with room to breathe.
    Comfortable,
}

/// The app-wide density, installed by [`crate::init`].
#[derive(Default)]
pub struct UiDensityStore {
    density: UiDensity,
}

impl Global for UiDensityStore {}

/// The density the app draws with right now.
pub fn ui_density(cx: &App) -> UiDensity {
    cx.try_global::<UiDensityStore>()
        .map(|store| store.density)
        .unwrap_or_default()
}

/// Change the density: everything that resolved a [`DynamicSpacing`] redraws at the new
/// size.
pub fn set_ui_density(cx: &mut App, density: UiDensity) {
    cx.global_mut::<UiDensityStore>().density = density;
}

/// A spacing step whose pixel value depends on [`UiDensity`].
///
/// The name says what the step is at the default density: `Base16` is 16 px wide when the
/// UI is at `Default`, 14 px when `Compact`, 18 px when `Comfortable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicSpacing {
    /// Nothing.
    Base00,
    /// Hairline gaps: 1/1/2.
    Base01,
    /// Tight inner padding: 1/2/4.
    Base02,
    /// Between a control and its label: 2/3/4.
    Base03,
    /// Control padding and row gaps: 2/4/6.
    Base04,
    /// Between related groups: 3/6/8.
    Base06,
    /// Section padding: 4/8/10.
    Base08,
    /// Large gaps inside a panel: 10/12/14.
    Base12,
    /// Between sections: 14/16/18.
    Base16,
    /// Between regions: 18/20/22.
    Base20,
    /// Fixed 24.
    Base24,
    /// Fixed 32.
    Base32,
    /// Fixed 40.
    Base40,
    /// Fixed 48.
    Base48,
}

impl DynamicSpacing {
    fn pixels_at(self, density: UiDensity) -> f32 {
        use UiDensity::{Comfortable, Compact, Default};
        match self {
            Self::Base00 => 0.,
            Self::Base01 => match density {
                Compact => 1.,
                Default => 1.,
                Comfortable => 2.,
            },
            Self::Base02 => match density {
                Compact => 1.,
                Default => 2.,
                Comfortable => 4.,
            },
            Self::Base03 => match density {
                Compact => 2.,
                Default => 3.,
                Comfortable => 4.,
            },
            Self::Base04 => match density {
                Compact => 2.,
                Default => 4.,
                Comfortable => 6.,
            },
            Self::Base06 => match density {
                Compact => 3.,
                Default => 6.,
                Comfortable => 8.,
            },
            Self::Base08 => match density {
                Compact => 4.,
                Default => 8.,
                Comfortable => 10.,
            },
            Self::Base12 => match density {
                Compact => 10.,
                Default => 12.,
                Comfortable => 14.,
            },
            Self::Base16 => match density {
                Compact => 14.,
                Default => 16.,
                Comfortable => 18.,
            },
            Self::Base20 => match density {
                Compact => 18.,
                Default => 20.,
                Comfortable => 22.,
            },
            Self::Base24 => 24.,
            Self::Base32 => 32.,
            Self::Base40 => 40.,
            Self::Base48 => 48.,
        }
    }

    /// This step's size for the density the app draws with.
    pub fn px(self, cx: &App) -> Pixels {
        px(self.pixels_at(ui_density(cx)))
    }

    /// This step's size as rems, so it scales with the UI font size and the display.
    pub fn rems(self, cx: &App) -> Rems {
        px_to_rems(self.pixels_at(ui_density(cx)))
    }
}
