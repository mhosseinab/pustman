//! The import every view file starts with: `use ui::prelude::*;`.
//!
//! It carries GPUI's own prelude and the element vocabulary a render method needs, the
//! design system's intents ([`Color`], [`TextSize`], [`DynamicSpacing`], [`StyledExt`]) and
//! every component, so a screen names *what* it draws instead of where that lives. Where
//! two libraries offer the same name, ours wins: [`KeyBinding`] is the keybinding *hint*
//! component, and `gpui::KeyBinding` is written out in full when a keymap is being built.

pub use gpui::prelude::*;
pub use gpui::{
    AbsoluteLength, AnyElement, App, ClickEvent, Context, CursorStyle, Div, ElementId, FocusHandle,
    Hsla, ParentElement, Pixels, Rems, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px, rems,
};

pub use theme::ActiveTheme;

pub use crate::actions::{Activate, Cancel, SelectNext, SelectPrevious};
pub use crate::color::{Color, Severity};
pub use crate::components::badge::Badge;
pub use crate::components::behavior::{
    Clickable, Disableable, FixedWidth, Toggleable, VisibleOnHover,
};
pub use crate::components::button::{
    Button, ButtonSize, ButtonStyle, ClickHandler, IconButton, IconButtonShape, TintColor,
};
pub use crate::components::context_menu::ContextMenu;
pub use crate::components::divider::{Divider, DividerColor};
pub use crate::components::key_binding::KeyBinding;
pub use crate::components::label::{Headline, Label, LabelSize};
pub use crate::components::list::VirtualList;
pub use crate::components::modal::Modal;
pub use crate::components::tab::Tab;
pub use crate::components::tab_bar::{TabBar, TabBarEvent};
pub use crate::components::table::{Table, TableColumn, TableRow};
pub use crate::components::toast::{Toast, ToastEvent, Toasts};
pub use crate::components::toggle::{Toggle, ToggleKind, ToggleState};
pub use crate::components::tooltip::{Tooltip, TooltipBuilder};
pub use crate::elevation::{ElevationIndex, StyledExt};
pub use crate::icons::{Icon, IconName, IconSize};
pub use crate::spacing::DynamicSpacing;
pub use crate::typography::{HeadlineSize, StyledTypography, TextSize};
pub use crate::units::px_to_rems;
pub use crate::{h_flex, v_flex};
