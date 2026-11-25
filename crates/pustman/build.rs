//! Embeds the app icon in the Windows executable as resource 1, the id GPUI's Windows
//! platform loads for the window, the taskbar and Alt-Tab; Explorer shows it too. Other
//! targets read their icon elsewhere (the macOS bundle, the Linux desktop entry), so this
//! is a no-op for them.

use std::error::Error;

const ICON_RESOURCE: &str = "../../assets/pustman-icon-pack/windows/app-icon.rc";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={ICON_RESOURCE}");
    println!("cargo:rerun-if-changed=../../assets/pustman-icon-pack/windows/app-icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    embed_resource::compile(ICON_RESOURCE, embed_resource::NONE).manifest_optional()?;
    Ok(())
}
