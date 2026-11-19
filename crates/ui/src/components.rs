//! The components a screen is composed from: buttons and icon buttons, labels, tabs,
//! tables, toggles, tooltips and the overlays (modal, toast).
//!
//! One file is one component or one family, each a value with a builder API that draws
//! itself through [`gpui::RenderOnce`] (`references/components.md`). Screens import them
//! from [`crate::prelude`]; the behavior several of them share lives in [`behavior`].

pub mod badge;
pub mod behavior;
pub mod button;
pub mod divider;
pub mod key_binding;
pub mod label;
pub mod modal;
pub mod tab;
pub mod tab_bar;
pub mod table;
pub mod toast;
pub mod toggle;
pub mod tooltip;
