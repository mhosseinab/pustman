//! The icon set: 16x16 SVGs derived from Lucide (ISC), embedded in the binary.
//!
//! `assets/icons/` holds the art (see `assets/icons/LICENSE` for the license and how the
//! Lucide 24x24 paths are mapped into Pustman's icon geometry). Icons are embedded with
//! [`IconAssets`], an [`gpui::AssetSource`] the app installs at startup
//! ([`crate::assets`]); [`Icon`] draws the same bytes directly so an icon renders
//! identically in a test, where GPUI installs an empty asset source and offers no way to
//! replace it (`references/design-system.md` §6).

use std::borrow::Cow;

use gpui::{App, AssetSource, IntoElement, RenderOnce, Result, SharedString, Styled, Window, svg};

use crate::color::Color;
use crate::units::px_to_rems;

/// The icon a component draws. Names are the file names in `assets/icons/` in PascalCase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    /// To the right: forward in a flow.
    ArrowRight,
    /// A collection or the workspace book.
    Book,
    /// A `{{variable}}`.
    Braces,
    /// Done, or tick a checkbox.
    Check,
    /// Open a dropdown or collapsed row.
    ChevronDown,
    /// Back, or collapse.
    ChevronLeft,
    /// Forward, or expand a row.
    ChevronRight,
    /// Close a dropdown, or move up.
    ChevronUp,
    /// A status dot, or an empty checkbox.
    Circle,
    /// An error that needs attention.
    CircleAlert,
    /// A success result.
    CircleCheck,
    /// A failure result, or dismiss.
    CircleX,
    /// Copy from the clipboard.
    Clipboard,
    /// History and elapsed time.
    Clock,
    /// Raw or pretty code view.
    Code,
    /// A cookie in the jar: the cookies tables and the Manage Cookies dialog.
    Cookie,
    /// Copy to the clipboard.
    Copy,
    /// Save a response body to disk.
    Download,
    /// More actions in a row.
    Ellipsis,
    /// Open outside Pustman.
    ExternalLink,
    /// Reveal a masked secret.
    Eye,
    /// Mask a secret again.
    EyeOff,
    /// A file, e.g. a request body.
    File,
    /// Narrow a list or response.
    Filter,
    /// A folder in a collection.
    Folder,
    /// An expanded folder.
    FolderOpen,
    /// Git branch and sharing state.
    GitBranch,
    /// A URL or the network.
    Globe,
    /// The drag handle: reorder request tabs and table rows.
    GripVertical,
    /// The history panel.
    History,
    /// An informational message.
    Info,
    /// Variables and secrets.
    Key,
    /// Work in progress: the spinner.
    LoaderCircle,
    /// TLS verified, or locked.
    Lock,
    /// Open the main menu.
    Menu,
    /// Collapse, or remove.
    Minus,
    /// Toggle the left dock.
    PanelLeft,
    /// Hold the console log.
    Pause,
    /// Rename or edit.
    Pencil,
    /// Pin a tab or entry.
    Pin,
    /// Run a request or a collection.
    Play,
    /// Add: tab, row, folder.
    Plus,
    /// Replay or refresh.
    RotateCw,
    /// Save to the workspace.
    Save,
    /// Search, quick open.
    Search,
    /// Send a request.
    Send,
    /// Settings.
    Settings,
    /// Environments and request options.
    SlidersHorizontal,
    /// Maximize or restore the window.
    Square,
    /// The console panel.
    Terminal,
    /// Delete.
    Trash,
    /// A warning, e.g. TLS verification off.
    TriangleAlert,
    /// TLS unverified, or unlocked.
    Unlock,
    /// Import a file.
    Upload,
    /// Offline or unreachable host.
    WifiOff,
    /// Close a tab, dialog or toast.
    X,
    /// Quick actions in the status bar (PRD §2 sketch).
    Zap,
}

/// Every icon, in the order the [`IconAssets::list`] reports them.
pub const ALL: &[IconName] = &[
    IconName::ArrowRight,
    IconName::Book,
    IconName::Braces,
    IconName::Check,
    IconName::ChevronDown,
    IconName::ChevronLeft,
    IconName::ChevronRight,
    IconName::ChevronUp,
    IconName::Circle,
    IconName::CircleAlert,
    IconName::CircleCheck,
    IconName::CircleX,
    IconName::Clipboard,
    IconName::Clock,
    IconName::Code,
    IconName::Cookie,
    IconName::Copy,
    IconName::Download,
    IconName::Ellipsis,
    IconName::ExternalLink,
    IconName::Eye,
    IconName::EyeOff,
    IconName::File,
    IconName::Filter,
    IconName::Folder,
    IconName::FolderOpen,
    IconName::GitBranch,
    IconName::Globe,
    IconName::GripVertical,
    IconName::History,
    IconName::Info,
    IconName::Key,
    IconName::LoaderCircle,
    IconName::Lock,
    IconName::Menu,
    IconName::Minus,
    IconName::PanelLeft,
    IconName::Pause,
    IconName::Pencil,
    IconName::Pin,
    IconName::Play,
    IconName::Plus,
    IconName::RotateCw,
    IconName::Save,
    IconName::Search,
    IconName::Send,
    IconName::Settings,
    IconName::SlidersHorizontal,
    IconName::Square,
    IconName::Terminal,
    IconName::Trash,
    IconName::TriangleAlert,
    IconName::Unlock,
    IconName::Upload,
    IconName::WifiOff,
    IconName::X,
    IconName::Zap,
];

impl IconName {
    /// Where this icon is embedded, as an asset path: `icons/send.svg`.
    pub fn path(self) -> &'static str {
        match self {
            Self::ArrowRight => "icons/arrow_right.svg",
            Self::Book => "icons/book.svg",
            Self::Braces => "icons/braces.svg",
            Self::Check => "icons/check.svg",
            Self::ChevronDown => "icons/chevron_down.svg",
            Self::ChevronLeft => "icons/chevron_left.svg",
            Self::ChevronRight => "icons/chevron_right.svg",
            Self::ChevronUp => "icons/chevron_up.svg",
            Self::Circle => "icons/circle.svg",
            Self::CircleAlert => "icons/circle_alert.svg",
            Self::CircleCheck => "icons/circle_check.svg",
            Self::CircleX => "icons/circle_x.svg",
            Self::Clipboard => "icons/clipboard.svg",
            Self::Clock => "icons/clock.svg",
            Self::Code => "icons/code.svg",
            Self::Cookie => "icons/cookie.svg",
            Self::Copy => "icons/copy.svg",
            Self::Download => "icons/download.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::ExternalLink => "icons/external_link.svg",
            Self::Eye => "icons/eye.svg",
            Self::EyeOff => "icons/eye_off.svg",
            Self::File => "icons/file.svg",
            Self::Filter => "icons/filter.svg",
            Self::Folder => "icons/folder.svg",
            Self::FolderOpen => "icons/folder_open.svg",
            Self::GitBranch => "icons/git_branch.svg",
            Self::Globe => "icons/globe.svg",
            Self::GripVertical => "icons/grip_vertical.svg",
            Self::History => "icons/history.svg",
            Self::Info => "icons/info.svg",
            Self::Key => "icons/key.svg",
            Self::LoaderCircle => "icons/loader_circle.svg",
            Self::Lock => "icons/lock.svg",
            Self::Menu => "icons/menu.svg",
            Self::Minus => "icons/minus.svg",
            Self::PanelLeft => "icons/panel_left.svg",
            Self::Pause => "icons/pause.svg",
            Self::Pencil => "icons/pencil.svg",
            Self::Pin => "icons/pin.svg",
            Self::Play => "icons/play.svg",
            Self::Plus => "icons/plus.svg",
            Self::RotateCw => "icons/rotate_cw.svg",
            Self::Save => "icons/save.svg",
            Self::Search => "icons/search.svg",
            Self::Send => "icons/send.svg",
            Self::Settings => "icons/settings.svg",
            Self::SlidersHorizontal => "icons/sliders_horizontal.svg",
            Self::Square => "icons/square.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::Trash => "icons/trash.svg",
            Self::TriangleAlert => "icons/triangle_alert.svg",
            Self::Unlock => "icons/unlock.svg",
            Self::Upload => "icons/upload.svg",
            Self::WifiOff => "icons/wifi_off.svg",
            Self::X => "icons/x.svg",
            Self::Zap => "icons/zap.svg",
        }
    }

    /// The icon's SVG source, as embedded in the binary.
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::ArrowRight => include_bytes!("../../../assets/icons/arrow_right.svg"),
            Self::Book => include_bytes!("../../../assets/icons/book.svg"),
            Self::Braces => include_bytes!("../../../assets/icons/braces.svg"),
            Self::Check => include_bytes!("../../../assets/icons/check.svg"),
            Self::ChevronDown => include_bytes!("../../../assets/icons/chevron_down.svg"),
            Self::ChevronLeft => include_bytes!("../../../assets/icons/chevron_left.svg"),
            Self::ChevronRight => include_bytes!("../../../assets/icons/chevron_right.svg"),
            Self::ChevronUp => include_bytes!("../../../assets/icons/chevron_up.svg"),
            Self::Circle => include_bytes!("../../../assets/icons/circle.svg"),
            Self::CircleAlert => include_bytes!("../../../assets/icons/circle_alert.svg"),
            Self::CircleCheck => include_bytes!("../../../assets/icons/circle_check.svg"),
            Self::CircleX => include_bytes!("../../../assets/icons/circle_x.svg"),
            Self::Clipboard => include_bytes!("../../../assets/icons/clipboard.svg"),
            Self::Clock => include_bytes!("../../../assets/icons/clock.svg"),
            Self::Code => include_bytes!("../../../assets/icons/code.svg"),
            Self::Cookie => include_bytes!("../../../assets/icons/cookie.svg"),
            Self::Copy => include_bytes!("../../../assets/icons/copy.svg"),
            Self::Download => include_bytes!("../../../assets/icons/download.svg"),
            Self::Ellipsis => include_bytes!("../../../assets/icons/ellipsis.svg"),
            Self::ExternalLink => include_bytes!("../../../assets/icons/external_link.svg"),
            Self::Eye => include_bytes!("../../../assets/icons/eye.svg"),
            Self::EyeOff => include_bytes!("../../../assets/icons/eye_off.svg"),
            Self::File => include_bytes!("../../../assets/icons/file.svg"),
            Self::Filter => include_bytes!("../../../assets/icons/filter.svg"),
            Self::Folder => include_bytes!("../../../assets/icons/folder.svg"),
            Self::FolderOpen => include_bytes!("../../../assets/icons/folder_open.svg"),
            Self::GitBranch => include_bytes!("../../../assets/icons/git_branch.svg"),
            Self::Globe => include_bytes!("../../../assets/icons/globe.svg"),
            Self::GripVertical => include_bytes!("../../../assets/icons/grip_vertical.svg"),
            Self::History => include_bytes!("../../../assets/icons/history.svg"),
            Self::Info => include_bytes!("../../../assets/icons/info.svg"),
            Self::Key => include_bytes!("../../../assets/icons/key.svg"),
            Self::LoaderCircle => include_bytes!("../../../assets/icons/loader_circle.svg"),
            Self::Lock => include_bytes!("../../../assets/icons/lock.svg"),
            Self::Menu => include_bytes!("../../../assets/icons/menu.svg"),
            Self::Minus => include_bytes!("../../../assets/icons/minus.svg"),
            Self::PanelLeft => include_bytes!("../../../assets/icons/panel_left.svg"),
            Self::Pause => include_bytes!("../../../assets/icons/pause.svg"),
            Self::Pencil => include_bytes!("../../../assets/icons/pencil.svg"),
            Self::Pin => include_bytes!("../../../assets/icons/pin.svg"),
            Self::Play => include_bytes!("../../../assets/icons/play.svg"),
            Self::Plus => include_bytes!("../../../assets/icons/plus.svg"),
            Self::RotateCw => include_bytes!("../../../assets/icons/rotate_cw.svg"),
            Self::Save => include_bytes!("../../../assets/icons/save.svg"),
            Self::Search => include_bytes!("../../../assets/icons/search.svg"),
            Self::Send => include_bytes!("../../../assets/icons/send.svg"),
            Self::Settings => include_bytes!("../../../assets/icons/settings.svg"),
            Self::SlidersHorizontal => {
                include_bytes!("../../../assets/icons/sliders_horizontal.svg")
            }
            Self::Square => include_bytes!("../../../assets/icons/square.svg"),
            Self::Terminal => include_bytes!("../../../assets/icons/terminal.svg"),
            Self::Trash => include_bytes!("../../../assets/icons/trash.svg"),
            Self::TriangleAlert => include_bytes!("../../../assets/icons/triangle_alert.svg"),
            Self::Unlock => include_bytes!("../../../assets/icons/unlock.svg"),
            Self::Upload => include_bytes!("../../../assets/icons/upload.svg"),
            Self::WifiOff => include_bytes!("../../../assets/icons/wifi_off.svg"),
            Self::X => include_bytes!("../../../assets/icons/x.svg"),
            Self::Zap => include_bytes!("../../../assets/icons/zap.svg"),
        }
    }

    /// The icon served at `path`, if that path is one of ours.
    pub fn from_path(path: &str) -> Option<Self> {
        ALL.iter().copied().find(|name| name.path() == path)
    }
}

/// How big an icon is drawn. [`IconSize::Medium`] is the 16 px the art is drawn for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconSize {
    /// 12 px: next to XSmall text.
    XSmall,
    /// 14 px: next to small text.
    Small,
    /// 16 px: the icon's own size.
    #[default]
    Medium,
    /// 20 px: empty states and headlines.
    Large,
}

impl IconSize {
    /// The side of the icon square.
    pub fn rems(self) -> gpui::Rems {
        px_to_rems(match self {
            Self::XSmall => 12.,
            Self::Small => 14.,
            Self::Medium => 16.,
            Self::Large => 20.,
        })
    }
}

/// An icon, tinted with a [`Color`] from the theme. Icon-only controls wrap this in an
/// [`crate::IconButton`], which carries the tooltip and the accessible label.
#[derive(Clone, Debug, IntoElement)]
pub struct Icon {
    name: IconName,
    size: IconSize,
    color: Color,
}

impl Icon {
    /// An icon at [`IconSize::Medium`] in [`Color::Default`].
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: IconSize::Medium,
            color: Color::Default,
        }
    }

    /// Draw it this big.
    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// Tint it with this intent. Resolved against the theme at render time.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// The icon this draws.
    pub fn name(&self) -> IconName {
        self.name
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // `data()` draws from the embedded bytes and derives its own cache key; GPUI has
        // no public way to name it after the asset path, so the bytes decide.
        svg()
            .data(self.name.bytes())
            .size(self.size.rems())
            .text_color(self.color.color(cx))
    }
}

/// The embedded icon set as an [`gpui::AssetSource`]: `icons/<name>.svg` resolves to the
/// SVG compiled into the binary, so nothing is read from disk at runtime.
///
/// The app installs it once with `gpui_platform::application().with_assets(ui::assets())`
/// (see [`crate::assets`]); it is also what serves `svg().path("icons/…")` anywhere in the
/// app.
#[derive(Clone, Copy, Debug, Default)]
pub struct IconAssets;

impl AssetSource for IconAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(IconName::from_path(path).map(|name| Cow::Borrowed(name.bytes())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path != "icons" {
            return Ok(Vec::new());
        }
        Ok(ALL
            .iter()
            .map(|name| SharedString::new_static(name.path()))
            .collect())
    }
}
