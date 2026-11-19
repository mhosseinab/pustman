//! [`ThemeRegistry`]: the themes the app can draw with, parsed from the JSON bundled in
//! `assets/themes/`. The files are embedded at compile time rather than read at runtime,
//! so the build ships the themes it was tested against and a missing file is a build
//! error instead of a blank window.

use std::sync::Arc;

use crate::{Appearance, Theme, ThemeError};

const LIGHT_JSON: &str = include_str!("../../../assets/themes/pustman-light.json");
const DARK_JSON: &str = include_str!("../../../assets/themes/pustman-dark.json");

/// The themes the app draws with: one per [`Appearance`], so `system` is a lookup that
/// cannot miss however the window appearance changes.
pub struct ThemeRegistry {
    light: Arc<Theme>,
    dark: Arc<Theme>,
}

impl ThemeRegistry {
    /// The two themes in `assets/themes/`, parsed here and never again.
    ///
    /// # Errors
    ///
    /// [`ThemeError::Parse`] naming the file that did not parse. The caller decides what
    /// to show; an app cannot draw without colors, so this must not be swallowed.
    pub fn bundled() -> Result<Self, ThemeError> {
        Ok(Self {
            light: parse("pustman-light", LIGHT_JSON)?,
            dark: parse("pustman-dark", DARK_JSON)?,
        })
    }

    /// Every bundled theme — what a check that must cover all of them iterates.
    pub fn themes(&self) -> [&Arc<Theme>; 2] {
        [&self.light, &self.dark]
    }

    /// The theme written for this appearance.
    pub fn for_appearance(&self, appearance: Appearance) -> &Arc<Theme> {
        match appearance {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        }
    }

    /// The theme with this name, for a theme picker or a test that names what it wants.
    pub fn get(&self, name: &str) -> Option<&Arc<Theme>> {
        self.themes().into_iter().find(|theme| theme.name() == name)
    }
}

fn parse(file: &str, json: &str) -> Result<Arc<Theme>, ThemeError> {
    let theme = serde_json::from_str::<Theme>(json).map_err(|error| ThemeError::Parse {
        name: file.to_string(),
        message: error.to_string(),
    })?;
    Ok(Arc::new(theme))
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
