//! The components a screen is composed from: buttons and icon buttons, labels, tabs,
//! tables, toggles, tooltips and the overlays (modal, toast).
//!
//! One file is one component or one family, each a value with a builder API that draws
//! itself through [`gpui::RenderOnce`] (`references/components.md`). Screens import them
//! from [`crate::prelude`]; the behavior several of them share lives in [`behavior`].
//! The one exception is [`context_menu`]: a menu owns its selection and focus, so it is a
//! [`gpui::Render`] entity like [`tab_bar`].

pub mod badge;
pub mod banner;
pub mod behavior;
pub mod button;
pub mod context_menu;
pub mod divider;
pub mod empty_state;
pub mod form_field;
pub mod key_binding;
pub mod label;
pub mod list;
pub mod modal;
pub mod tab;
pub mod tab_bar;
pub mod table;
pub mod toast;
pub mod toggle;
pub mod tooltip;
