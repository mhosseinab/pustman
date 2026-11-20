# Rust API desktop icon pack

Combined design: the Rust-inspired crab from option 2, with the opposing arrow motif from option 1 forming its claws. The final artwork is a clean, symmetrical vector redraw of the combined concept.

## Start here

| Use | File or folder |
| --- | --- |
| Edit the icon | `source/app-icon.svg` |
| Use the symbol without the tile | `source/app-symbol.svg` |
| Windows application icon | `windows/app-icon.ico` |
| macOS application icon | `macos/app-icon.icns` |
| macOS iconutil input | `macos/app-icon.iconset/` |
| Xcode asset catalog input | `macos/AppIcon.appiconset/` |
| Ubuntu / Linux application icons | `linux/hicolor/` |
| Individual PNGs | `png/` |
| Optional Tauri layout | `tauri/icons/` |
| Visual size comparison | `preview.png` |
| Dimensions, validation results and checksums | `manifest.json` |

`Rust API` is a descriptive placeholder, not a chosen product name. Rename the files and application identifiers to match your app when integrating them.

## Editable source

The master has a `1024 × 1024` viewBox. Its graphite tile is `896 × 896`, placed at `(64, 64)`, with a corner radius of `176`. The outer margin is transparent.

| Element | Color |
| --- | --- |
| Background tile | `#191C25` |
| Crab body and legs | `#FF7548` |
| Arrow claws | `#FFF8EE` |

The SVG contains editable paths, stroked paths and a rectangle, with named groups for `background`, `symbol`, `claws`, `legs` and `body`. The eyes are real transparent cutouts. There is no embedded bitmap, text/font dependency, texture, external asset or filter. The generated image's minor asymmetries and raster texture were deliberately replaced with clean geometry.

Every PNG is rendered from SVG at its intended size. The build does not enlarge or repeatedly downsample a raster master. Smooth edge antialiasing is expected; pixel-perfect does not mean disabling antialiasing on curves or diagonal arrows.

## PNG sizes

All are square, 8-bit RGBA PNGs with transparent outer padding:

`16, 20, 22, 24, 28, 30, 32, 36, 40, 44, 48, 50, 60, 64, 72, 80, 96, 128, 144, 192, 256, 310, 384, 512, 768, 1024, 2048` pixels.

The 2048px PNG is an extra large export. Use the SVG when another size is needed.

## Windows

`windows/app-icon.ico` embeds these 15 sizes in a single file:

`16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256` pixels.

All entries use 32-bit color with alpha. Entries below 256px are uncompressed Windows DIBs with transparency masks; the 256px entry is PNG-compressed. These include Microsoft's stated minimum set and the sizes in its Windows 11 scaling table. [Microsoft icon construction guidance](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-construction).

Use this ICO as the executable's application icon and for the framework's window icon resource. A minimal Windows resource fragment is included in `windows/app-icon.rc`; integrate it with the resource build process used by your Rust application.

This is the conventional desktop icon pack. MSIX/UWP manifest images, Microsoft Store artwork, installer banners, and taskbar overlay/status badges are separate assets and are not claimed to be covered by this ICO. Microsoft documents those separately on the same page.

## macOS

The ICNS, iconset and Xcode appiconset contain all ten conventional macOS size/scale slots:

| Logical size | 1× image | 2× image |
| --- | --- | --- |
| 16pt | 16px | 32px |
| 32pt | 32px | 64px |
| 128pt | 128px | 256px |
| 256pt | 256px | 512px |
| 512pt | 512px | 1024px |

For a conventional app bundle, copy the ICNS into `YourApp.app/Contents/Resources/` and set `CFBundleIconFile` to `app-icon` in the app's existing `Info.plist`. A small plist fragment is included. If your framework manages this, configure the framework instead of editing a generated app bundle.

For Xcode, add `AppIcon.appiconset` to your existing asset catalog. For Apple's own ICNS conversion on a Mac, run:

```sh
iconutil -c icns macos/app-icon.iconset -o macos/app-icon.icns
```

The filenames and 1×/2× slots follow Apple's [Mac App Programming Guide](https://developer.apple.com/library/archive/documentation/General/Conceptual/MOSXAppProgrammingGuide/CommonAppBehaviors/CommonAppBehaviors.html) and [High Resolution Guidelines](https://developer.apple.com/library/archive/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Optimizing/Optimizing.html).

The supplied `.icns` and asset catalog are for the conventional desktop workflow. An Apple Icon Composer `.icon` project and compiled `Assets.car` are not included. The separate editable foreground SVG can be reused if you later adopt a layered icon workflow.

## Ubuntu / Linux

`linux/hicolor/` supplies PNGs at `16, 22, 24, 32, 36, 48, 64, 72, 96, 128, 192, 256, 512` pixels, plus `scalable/apps/app-icon.svg`.

Install the assets into the matching directories under your application's installation prefix, for example `share/icons/hicolor/48x48/apps/`. Give the icon a unique basename matching your app, and use that basename without an extension in the `.desktop` file's `Icon=` entry. The template `linux/app.desktop.in` shows the relevant fields; replace its `@...@` placeholders before installing it.

The pack does not replace the system's `hicolor/index.theme`. These directories are additions to the standard fallback theme. [Freedesktop Icon Theme Specification](https://specifications.freedesktop.org/icon-theme/latest/).

## Optional Tauri integration

If the Rust app uses Tauri, copy the contents of `tauri/icons/` into `src-tauri/icons/`. Merge the `bundle.icon` field from `tauri/bundle-icons.fragment.json` into your existing Tauri configuration; do not replace the whole configuration with that fragment.

The provided filenames are `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.ico` and `icon.icns`. The ICO stores its 32px entry first, as Tauri recommends for its Windows development icon. Tauri's own icon generator can also consume the master SVG if you need its additional outputs. [Tauri app icons documentation](https://v2.tauri.app/develop/icons/).

## Rebuild after editing

The build uses Python 3, Pillow and Inkscape available on `PATH`. The delivered pack was built with Inkscape 1.4 and Pillow 13.0.0. Install Python dependencies with:

```sh
python3 -m pip install -r requirements.txt
```

Then run:

```sh
python3 build_icons.py
```

This regenerates the PNGs, ICO, ICNS, iconset, appiconset, Linux and Tauri copies, preview and manifest in place. The script renders each target size directly from SVG and checks dimensions, alpha, ICO round trips, ICNS chunks and the Xcode catalog.

## Verification

The source was checked for embedded bitmap content and external dependencies. Every PNG was checked for its dimensions and transparency. Every ICO entry was decoded and compared pixel for pixel with the matching PNG. All ten ICNS chunks were checked for their type and image dimensions, and the container was decoded independently with Pillow. The Xcode catalog's filenames, dimensions and scale slots were checked. Small renders were visually inspected on light and dark backgrounds.

These are asset-level checks. The pack has not been installed inside your application or run on physical macOS or Windows systems, since no application project was supplied.
