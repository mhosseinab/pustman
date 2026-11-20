#!/usr/bin/env python3
"""Rebuild the desktop icon pack from editable SVG, using Inkscape and Pillow.

Run from any directory: python3 /path/to/rust-api-icon-pack/build_icons.py
Every size is rendered from SVG; raster outputs are never used as resize masters.
"""

from __future__ import annotations

import hashlib
import io
import json
import shutil
import struct
import subprocess
import sys
import xml.etree.ElementTree as ET
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parent
SOURCE = ROOT / "source" / "app-icon.svg"
PNG_SIZES = (16, 20, 22, 24, 28, 30, 32, 36, 40, 44, 48, 50, 60, 64,
             72, 80, 96, 128, 144, 192, 256, 310, 384, 512, 768, 1024, 2048)
ICO_SIZES = (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256)
LINUX_SIZES = (16, 22, 24, 32, 36, 48, 64, 72, 96, 128, 192, 256, 512)
MAC_SLOTS = (
    (16, 1, b"icp4"), (16, 2, b"ic11"),
    (32, 1, b"icp5"), (32, 2, b"ic12"),
    (128, 1, b"ic07"), (128, 2, b"ic13"),
    (256, 1, b"ic08"), (256, 2, b"ic14"),
    (512, 1, b"ic09"), (512, 2, b"ic10"),
)
SVG_NS = "http://www.w3.org/2000/svg"
ET.register_namespace("", SVG_NS)


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def copy_file(source: Path, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)


def png_path(size: int) -> Path:
    return ROOT / "png" / f"app-icon-{size}.png"


def render_svg(source: Path, target: Path, size: int, renderer: str) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    command = [renderer, str(source), "--export-type=png", "--export-area-page",
               f"--export-filename={target}", f"--export-width={size}",
               f"--export-height={size}", "--export-background-opacity=0"]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"SVG render failed at {size}px: {result.stderr}")
    with Image.open(target) as original:
        image = original.convert("RGBA")
        assert image.size == (size, size), (target, image.size)
        # Set invisible RGB to zero for clean edges in legacy icon readers.
        zero_alpha = image.getchannel("A").point(lambda a: 255 if a == 0 else 0)
        image.paste((0, 0, 0, 0), (0, 0, size, size), zero_alpha)
        image.save(target, "PNG", optimize=True)


def make_foreground_svg() -> None:
    """Preserve the editable foreground as an optional separate source layer."""
    tree = ET.parse(SOURCE)
    root = tree.getroot()
    for child in list(root):
        if child.get("id") == "background":
            root.remove(child)
    title = root.find(f"{{{SVG_NS}}}title")
    if title is not None:
        title.text = "Rust API client — foreground symbol"
    description = root.find(f"{{{SVG_NS}}}desc")
    if description is not None:
        description.text = "Editable crab and arrow symbol with transparent eye cutouts and no background."
    tree.write(ROOT / "source" / "app-symbol.svg", encoding="utf-8", xml_declaration=True)


def dib_payload(image: Image.Image) -> bytes:
    """32-bit bottom-up BGRA plus a padded 1-bit Windows transparency mask."""
    image = image.convert("RGBA")
    width, height = image.size
    color = image.tobytes("raw", "BGRA", 0, -1)
    stride = ((width + 31) // 32) * 4
    mask = bytearray(stride * height)
    alpha = image.getchannel("A")
    for output_y in range(height):
        input_y = height - 1 - output_y
        for x in range(width):
            if alpha.getpixel((x, input_y)) == 0:
                mask[output_y * stride + x // 8] |= 0x80 >> (x % 8)
    header = struct.pack("<IiiHHIIiiII", 40, width, height * 2, 1, 32,
                         0, len(color) + len(mask), 0, 0, 0, 0)
    return header + color + bytes(mask)


def make_ico(target: Path) -> None:
    # Tauri recommends a 32px first entry for its Windows development icon.
    ordered_sizes = (32,) + tuple(size for size in ICO_SIZES if size != 32)
    payloads = []
    for size in ordered_sizes:
        if size == 256:
            payloads.append(png_path(size).read_bytes())
        else:
            with Image.open(png_path(size)) as image:
                payloads.append(dib_payload(image))
    offset = 6 + 16 * len(ICO_SIZES)
    entries = bytearray()
    for size, payload in zip(ordered_sizes, payloads):
        entries.extend(struct.pack("<BBBBHHII", size % 256, size % 256,
                                   0, 0, 1, 32, len(payload), offset))
        offset += len(payload)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(struct.pack("<HHH", 0, 1, len(ICO_SIZES)) + entries + b"".join(payloads))


def make_icns(target: Path) -> None:
    chunks = []
    for points, scale, kind in MAC_SLOTS:
        payload = png_path(points * scale).read_bytes()
        chunks.append(kind + struct.pack(">I", 8 + len(payload)) + payload)
    toc = b"".join(chunk[:8] for chunk in chunks)
    toc_chunk = b"TOC " + struct.pack(">I", 8 + len(toc)) + toc
    body = toc_chunk + b"".join(chunks)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def make_macos_assets() -> None:
    images = []
    for points, scale, kind in MAC_SLOTS:
        suffix = "@2x" if scale == 2 else ""
        filename = f"icon_{points}x{points}{suffix}.png"
        source = png_path(points * scale)
        copy_file(source, ROOT / "macos" / "app-icon.iconset" / filename)
        copy_file(source, ROOT / "macos" / "AppIcon.appiconset" / filename)
        images.append({"idiom": "mac", "size": f"{points}x{points}",
                       "scale": f"{scale}x", "filename": filename})
    write_text(ROOT / "macos" / "AppIcon.appiconset" / "Contents.json",
               json.dumps({"images": images, "info": {"version": 1, "author": "xcode"}}, indent=2) + "\n")
    make_icns(ROOT / "macos" / "app-icon.icns")


def make_linux_assets() -> None:
    for size in LINUX_SIZES:
        copy_file(png_path(size), ROOT / "linux" / "hicolor" /
                  f"{size}x{size}" / "apps" / "app-icon.png")
    copy_file(SOURCE, ROOT / "linux" / "hicolor" / "scalable" / "apps" / "app-icon.svg")


def make_tauri_assets() -> None:
    for filename, size in (("32x32.png", 32), ("128x128.png", 128),
                           ("128x128@2x.png", 256), ("icon.png", 512)):
        copy_file(png_path(size), ROOT / "tauri" / "icons" / filename)
    copy_file(ROOT / "windows" / "app-icon.ico", ROOT / "tauri" / "icons" / "icon.ico")
    copy_file(ROOT / "macos" / "app-icon.icns", ROOT / "tauri" / "icons" / "icon.icns")
    write_text(ROOT / "tauri" / "bundle-icons.fragment.json",
               json.dumps({"bundle": {"icon": ["icons/32x32.png", "icons/128x128.png",
                           "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"]}}, indent=2) + "\n")


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    names = (["/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", "DejaVuSans-Bold.ttf"]
             if bold else ["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "DejaVuSans.ttf"])
    for name in names:
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            pass
    return ImageFont.load_default(size=size)


def make_preview() -> None:
    canvas = Image.new("RGBA", (1120, 720), "#F5F6F8")
    draw = ImageDraw.Draw(canvas)
    draw.text((40, 27), "Crab + request / response", fill="#191C25", font=font(27, True))
    draw.text((40, 67), "Editable vector master and desktop icon exports", fill="#667080", font=font(16))
    panels = [(32, "#FFFFFF", "#191C25", "LIGHT BACKGROUND"),
              (576, "#11141B", "#EDF0F5", "DARK BACKGROUND")]
    for x, background, foreground, label in panels:
        draw.rounded_rectangle((x, 112, x + 512, 658), radius=20, fill=background)
        draw.text((x + 28, 136), label, fill=foreground, font=font(12, True))
        with Image.open(png_path(256)) as icon:
            canvas.alpha_composite(icon.convert("RGBA"), (x + 128, 184))
        draw.text((x + 28, 467), "Actual pixel sizes", fill=foreground, font=font(14))
        sizes = (16, 24, 32, 48, 64, 128)
        cursor = x + 30
        for size in sizes:
            with Image.open(png_path(size)) as icon:
                canvas.alpha_composite(icon.convert("RGBA"), (cursor, 570 - size))
            text = str(size)
            width = draw.textlength(text, font=font(11))
            draw.text((cursor + (size - width) / 2, 584), text, fill=foreground, font=font(11))
            cursor += size + 18
    draw.text((40, 684), "SVG  /  PNG  /  ICO  /  ICNS     •     Transparent outer padding", fill="#667080", font=font(13))
    canvas.convert("RGB").save(ROOT / "preview.png", optimize=True)


def validate() -> dict:
    root = ET.parse(SOURCE).getroot()
    for element in root.iter():
        assert element.tag.rsplit("}", 1)[-1] not in {"image", "script", "foreignObject"}, "Non-vector source content"
        for attribute in ("href", f"{{http://www.w3.org/1999/xlink}}href"):
            assert not element.get(attribute), "External dependency in SVG"
    assert root.get("viewBox") == "0 0 1024 1024"
    for size in PNG_SIZES:
        with Image.open(png_path(size)) as image:
            assert image.size == (size, size)
            assert image.mode == "RGBA"
            assert image.getextrema()[3] == (0, 255)
            assert image.getpixel((0, 0))[3] == 0
            assert image.getpixel((size - 1, size - 1))[3] == 0
    ico_path = ROOT / "windows" / "app-icon.ico"
    with Image.open(ico_path) as ico:
        assert ico.ico.sizes() == {(s, s) for s in ICO_SIZES}
        for size in ICO_SIZES:
            actual = ico.ico.getimage((size, size)).convert("RGBA")
            with Image.open(png_path(size)) as expected:
                assert actual.tobytes() == expected.convert("RGBA").tobytes(), f"ICO round trip differs at {size}"
    assert ico_path.read_bytes()[6:8] == bytes((32, 32)), "ICO must start with the 32px entry"
    data = (ROOT / "macos" / "app-icon.icns").read_bytes()
    assert data[:4] == b"icns"
    assert struct.unpack(">I", data[4:8])[0] == len(data)
    offset, found = 8, {}
    while offset < len(data):
        kind = data[offset:offset + 4]
        length = struct.unpack(">I", data[offset + 4:offset + 8])[0]
        assert length >= 8 and offset + length <= len(data)
        if kind != b"TOC ":
            payload = data[offset + 8:offset + length]
            with Image.open(io.BytesIO(payload)) as image:
                image.load()
                found[kind] = image.size
        offset += length
    assert offset == len(data)
    assert found == {kind: (points * scale, points * scale) for points, scale, kind in MAC_SLOTS}
    with Image.open(ROOT / "macos" / "app-icon.icns") as icns:
        icns.load()
        assert icns.size == (1024, 1024)
    catalog = json.loads((ROOT / "macos" / "AppIcon.appiconset" / "Contents.json").read_text())
    assert len(catalog["images"]) == 10
    for record in catalog["images"]:
        points = int(record["size"].split("x")[0])
        scale = int(record["scale"][0])
        with Image.open(ROOT / "macos" / "AppIcon.appiconset" / record["filename"]) as image:
            assert image.size == (points * scale, points * scale)
    return {"svg_is_vector_only": True, "png_dimensions_and_alpha": "passed",
            "ico_frames_exactly_match_pngs": True, "icns_chunk_dimensions": "passed",
            "icns_decoder_load": "passed", "macos_asset_catalog": "passed",
            "native_os_installation_tested": False}


def main() -> None:
    renderer = shutil.which("inkscape")
    if not renderer:
        sys.exit("Inkscape is required. Install Inkscape and make its executable available on PATH.")
    if not SOURCE.exists():
        sys.exit(f"Missing editable source: {SOURCE}")
    make_foreground_svg()
    with ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(lambda size: render_svg(SOURCE, png_path(size), size, renderer), PNG_SIZES))
    make_ico(ROOT / "windows" / "app-icon.ico")
    make_macos_assets()
    make_linux_assets()
    make_tauri_assets()
    make_preview()
    validation = validate()
    manifest = {
        "name": "Rust API desktop icon pack",
        "design": "Combined Rust crab and opposing request / response arrow claws",
        "source": "source/app-icon.svg",
        "view_box": [0, 0, 1024, 1024],
        "palette": {"background": "#191C25", "crab": "#FF7548", "arrows": "#FFF8EE"},
        "png_sizes": list(PNG_SIZES), "ico_sizes": list(ICO_SIZES),
        "linux_sizes": list(LINUX_SIZES),
        "macos_slots": [{"points": p, "scale": s, "pixels": p*s, "chunk": k.decode()}
                        for p, s, k in MAC_SLOTS],
        "renderer": subprocess.check_output([renderer, "--version"], text=True).strip(),
        "validation": validation,
        "checksums_sha256": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                             for path in sorted(ROOT.rglob("*"))
                             if path.is_file() and path.name != "manifest.json" and "__pycache__" not in path.parts},
    }
    write_text(ROOT / "manifest.json", json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"status": "ok", "png_sizes": len(PNG_SIZES), "ico_frames": len(ICO_SIZES),
                      "icns_slots": len(MAC_SLOTS), "validation": validation}, indent=2))


if __name__ == "__main__":
    main()
