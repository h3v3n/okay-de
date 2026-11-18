#!/usr/bin/env python3
"""
Build script for the `okay-cursors` cursor theme (OKay DE).

Generates:
  - Vector SVG sources (`src/svg/*.svg`, viewBox="0 0 24 24")
  - Multi-size PNG renders (`src/png/{24,32,48}/*.png`) via Rsvg + Cairo
  - xcursorgen configuration files (`src/config/*.cursor`)
  - Binary Xcursor files (`cursors/*`) + full X11/Wayland/Qt symlinks
  - `index.theme` and `cursor.theme`

Style: Premium Dark Glass & Anodized Metal (#0F172A -> #1E293B),
crisp specular rim (rgba(255,255,255,0.25)), soft drop shadow,
and Oksavi emerald accents (#10B981 / #34D399).
"""

from __future__ import annotations

import math
import os
import shutil
import struct
import subprocess
from pathlib import Path

import cairo
import gi

gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg

SIZES = (24, 32, 48)
ANIM_FRAMES = 16
ANIM_DELAY_MS = 50

BASE_DIR = Path(__file__).resolve().parent / "okay-cursors"
SVG_DIR = BASE_DIR / "src" / "svg"
PNG_DIR = BASE_DIR / "src" / "png"
CFG_DIR = BASE_DIR / "src" / "config"
CUR_DIR = BASE_DIR / "cursors"


SHARED_DEFS = """
  <defs>
    <linearGradient id="darkGlass" x1="0%" y1="0%" x2="85%" y2="100%">
      <stop offset="0%" stop-color="#1E293B"/>
      <stop offset="55%" stop-color="#131C2E"/>
      <stop offset="100%" stop-color="#0F172A"/>
    </linearGradient>
    <linearGradient id="glassSheen" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="rgba(255,255,255,0.28)"/>
      <stop offset="55%" stop-color="rgba(255,255,255,0.06)"/>
      <stop offset="100%" stop-color="rgba(255,255,255,0.0)"/>
    </linearGradient>
    <linearGradient id="anodizedRim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="rgba(255,255,255,0.48)"/>
      <stop offset="45%" stop-color="rgba(255,255,255,0.25)"/>
      <stop offset="100%" stop-color="rgba(148,163,184,0.28)"/>
    </linearGradient>
    <linearGradient id="emeraldGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#34D399"/>
      <stop offset="100%" stop-color="#10B981"/>
    </linearGradient>
    <radialGradient id="emeraldGlow" cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="rgba(16,185,129,0.55)"/>
      <stop offset="65%" stop-color="rgba(16,185,129,0.14)"/>
      <stop offset="100%" stop-color="rgba(16,185,129,0)"/>
    </radialGradient>
    <linearGradient id="dangerGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#FB7185"/>
      <stop offset="100%" stop-color="#E11D48"/>
    </linearGradient>
    <filter id="softShadow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="0.85"/>
      <feOffset dx="0.45" dy="0.95" result="offsetblur"/>
      <feComponentTransfer>
        <feFuncA type="linear" slope="0.58"/>
      </feComponentTransfer>
      <feMerge>
        <feMergeNode/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>
  </defs>
"""


def wrap_svg(body: str) -> str:
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" '
        'width="24" height="24">\n'
        f"{SHARED_DEFS}\n"
        f"{body}\n"
        "</svg>\n"
    )


def pointer_arrow_group(with_emerald_tail: bool = True) -> str:
    # Sharp geometric pointer whose tip lands cleanly on (2, 2).
    # Body stays within [2..15, 2..19] to leave breathing room for softShadow.
    arrow_path = "M 2 2 L 2 16.5 L 5.8 13.0 L 8.4 18.6 L 10.6 17.6 L 8.0 12.1 L 13.1 11.7 Z"
    accent = ""
    if with_emerald_tail:
        accent = (
            '    <path d="M 6.0 12.5 L 8.5 17.9 L 10.0 17.2 L 7.5 11.9 Z" '
            'fill="url(#emeraldGrad)" opacity="0.85"/>\n'
            '    <circle cx="2.7" cy="3.2" r="2.2" fill="url(#emeraldGlow)"/>\n'
            '    <circle cx="2.35" cy="2.55" r="0.55" fill="#34D399" opacity="0.95"/>\n'
        )
    return (
        '  <g filter="url(#softShadow)">\n'
        f'    <path d="{arrow_path}" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.28)" stroke-width="0.95" '
        'stroke-linejoin="round"/>\n'
        f'    <path d="{arrow_path}" fill="url(#glassSheen)" '
        'stroke="url(#anodizedRim)" stroke-width="0.55" stroke-linejoin="round"/>\n'
        f"{accent}"
        "  </g>"
    )


def svg_default() -> str:
    return wrap_svg(pointer_arrow_group(with_emerald_tail=True))


def svg_pointer() -> str:
    # Anodized pointing hand; index finger center at x=9.0, top tip at y=2.0 -> hotspot (9, 2).
    hand_path = (
        "M 9.0 2.0 "
        "C 9.95 2.0 10.55 2.75 10.55 3.7 L 10.55 9.0 "
        "C 11.0 8.55 12.2 8.45 12.75 9.1 "
        "C 13.35 8.75 14.5 8.85 15.0 9.55 "
        "C 15.65 9.3 16.65 9.65 16.85 10.65 "
        "L 16.85 14.8 "
        "C 16.85 17.35 15.15 19.3 12.6 19.55 "
        "L 9.2 19.55 "
        "C 7.55 19.55 6.45 18.45 5.65 16.8 "
        "L 4.1 13.55 "
        "C 3.65 12.65 4.2 11.7 5.05 11.6 "
        "C 5.75 11.52 6.4 11.98 6.85 12.75 L 7.45 13.7 "
        "L 7.45 3.7 C 7.45 2.75 8.05 2.0 9.0 2.0 Z"
    )
    return wrap_svg(
        '  <g filter="url(#softShadow)">\n'
        f'    <path d="{hand_path}" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.28)" stroke-width="0.95" stroke-linejoin="round"/>\n'
        f'    <path d="{hand_path}" fill="url(#glassSheen)" '
        'stroke="url(#anodizedRim)" stroke-width="0.5" stroke-linejoin="round"/>\n'
        '    <line x1="10.6" y1="10.0" x2="10.6" y2="12.5" stroke="rgba(255,255,255,0.18)" stroke-width="0.65" stroke-linecap="round"/>\n'
        '    <line x1="12.8" y1="10.2" x2="12.8" y2="12.6" stroke="rgba(255,255,255,0.18)" stroke-width="0.65" stroke-linecap="round"/>\n'
        '    <line x1="14.9" y1="10.6" x2="14.9" y2="12.9" stroke="rgba(255,255,255,0.16)" stroke-width="0.65" stroke-linecap="round"/>\n'
        '    <rect x="8.6" y="17.3" width="5.4" height="1.35" rx="0.65" fill="url(#emeraldGrad)"/>\n'
        '    <circle cx="9.0" cy="3.1" r="1.9" fill="url(#emeraldGlow)"/>\n'
        '    <circle cx="9.0" cy="3.1" r="0.6" fill="#34D399"/>\n'
        "  </g>"
    )


def svg_grab(closed: bool = False) -> str:
    if not closed:
        # Open palm hand centered near (12, 12)
        path = (
            "M 7.1 9.8 L 7.1 5.6 C 7.1 4.9 8.4 4.9 8.4 5.6 L 8.4 9.5 "
            "L 9.5 4.2 C 9.5 3.4 10.9 3.4 10.9 4.2 L 10.9 9.2 "
            "L 12.1 3.9 C 12.1 3.1 13.5 3.1 13.5 3.9 L 13.5 9.3 "
            "L 14.7 5.0 C 14.7 4.2 16.0 4.2 16.0 5.0 L 16.0 10.2 "
            "L 17.5 8.8 C 18.2 8.2 19.1 9.0 18.6 9.9 L 16.8 14.3 "
            "C 16.1 16.9 14.2 18.9 11.7 19.0 L 9.5 19.0 "
            "C 7.4 19.0 6.1 17.1 5.6 14.4 L 5.1 11.4 "
            "C 4.9 10.4 6.1 9.8 7.1 9.8 Z"
        )
        lines = (
            '    <rect x="8.3" y="16.8" width="5.6" height="1.3" rx="0.65" fill="url(#emeraldGrad)"/>\n'
            '    <circle cx="11.5" cy="12.2" r="2.2" fill="url(#emeraldGlow)"/>\n'
        )
    else:
        # Closed fist gripping hand centered near (12, 12)
        path = (
            "M 6.8 10.2 C 6.8 9.0 8.2 8.7 8.8 9.5 "
            "C 9.1 8.3 10.6 8.2 11.2 9.1 "
            "C 11.7 8.1 13.2 8.2 13.7 9.2 "
            "C 14.3 8.4 15.8 8.7 16.0 9.9 "
            "L 16.5 13.8 C 16.5 16.7 14.6 18.8 12.0 18.9 L 9.6 18.9 "
            "C 7.5 18.9 6.0 17.1 5.8 14.5 L 5.6 12.0 C 5.5 10.8 6.2 10.2 6.8 10.2 Z"
        )
        lines = (
            '    <line x1="8.9" y1="9.9" x2="8.9" y2="12.5" stroke="rgba(255,255,255,0.22)" stroke-width="0.65" stroke-linecap="round"/>\n'
            '    <line x1="11.3" y1="9.6" x2="11.3" y2="12.5" stroke="rgba(255,255,255,0.22)" stroke-width="0.65" stroke-linecap="round"/>\n'
            '    <line x1="13.7" y1="9.8" x2="13.7" y2="12.5" stroke="rgba(255,255,255,0.22)" stroke-width="0.65" stroke-linecap="round"/>\n'
            '    <rect x="8.3" y="16.7" width="5.4" height="1.35" rx="0.65" fill="url(#emeraldGrad)"/>\n'
            '    <circle cx="11.2" cy="13.8" r="1.2" fill="#34D399" opacity="0.9"/>\n'
        )
    return wrap_svg(
        '  <g filter="url(#softShadow)">\n'
        f'    <path d="{path}" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.28)" stroke-width="0.95" stroke-linejoin="round"/>\n'
        f'    <path d="{path}" fill="url(#glassSheen)" '
        'stroke="url(#anodizedRim)" stroke-width="0.5" stroke-linejoin="round"/>\n'
        f"{lines}"
        "  </g>"
    )


def svg_text() -> str:
    # Vertical I-beam centered on (12, 12)
    ibeam = (
        "M 8.2 4.2 L 15.8 4.2 L 15.8 6.1 L 13.05 6.4 "
        "L 13.05 17.6 L 15.8 17.9 L 15.8 19.8 L 8.2 19.8 "
        "L 8.2 17.9 L 10.95 17.6 L 10.95 6.4 L 8.2 6.1 Z"
    )
    return wrap_svg(
        '  <g filter="url(#softShadow)">\n'
        f'    <path d="{ibeam}" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.32)" stroke-width="0.9" stroke-linejoin="round"/>\n'
        f'    <path d="{ibeam}" fill="url(#glassSheen)" '
        'stroke="url(#anodizedRim)" stroke-width="0.45" stroke-linejoin="round"/>\n'
        '    <rect x="11.35" y="6.8" width="1.3" height="10.4" rx="0.65" fill="rgba(226,232,240,0.45)"/>\n'
        '    <rect x="10.3" y="11.35" width="3.4" height="1.3" rx="0.65" fill="url(#emeraldGrad)"/>\n'
        '    <circle cx="12" cy="12" r="2.2" fill="url(#emeraldGlow)"/>\n'
        "  </g>"
    )


def svg_crosshair() -> str:
    # Precision reticle centered strictly at (12, 12)
    return wrap_svg(
        '  <g filter="url(#softShadow)">\n'
        '    <circle cx="12" cy="12" r="6.2" fill="none" stroke="#0F172A" stroke-width="2.5"/>\n'
        '    <circle cx="12" cy="12" r="6.2" fill="none" stroke="rgba(255,255,255,0.34)" stroke-width="0.9"/>\n'
        '    <path d="M 12 2.4 L 12 8.2 M 12 15.8 L 12 21.6 M 2.4 12 L 8.2 12 M 15.8 12 L 21.6 12" '
        'stroke="#0F172A" stroke-width="2.6" stroke-linecap="round"/>\n'
        '    <path d="M 12 2.4 L 12 8.2 M 12 15.8 L 12 21.6 M 2.4 12 L 8.2 12 M 15.8 12 L 21.6 12" '
        'stroke="rgba(255,255,255,0.38)" stroke-width="1.05" stroke-linecap="round"/>\n'
        '    <circle cx="12" cy="12" r="2.6" fill="url(#emeraldGlow)"/>\n'
        '    <circle cx="12" cy="12" r="1.25" fill="url(#emeraldGrad)" stroke="rgba(255,255,255,0.4)" stroke-width="0.45"/>\n'
        "  </g>"
    )


def svg_move() -> str:
    # 4-way anodized arrow hub centered on (12, 12)
    four_way = (
        "M 12 2.2 L 14.9 5.5 L 13.15 5.5 L 13.15 10.85 "
        "L 18.5 10.85 L 18.5 9.1 L 21.8 12 L 18.5 14.9 L 18.5 13.15 "
        "L 13.15 13.15 L 13.15 18.5 L 14.9 18.5 L 12 21.8 "
        "L 9.1 18.5 L 10.85 18.5 L 10.85 13.15 "
        "L 5.5 13.15 L 5.5 14.9 L 2.2 12 L 5.5 9.1 L 5.5 10.85 "
        "L 10.85 10.85 L 10.85 5.5 L 9.1 5.5 Z"
    )
    return wrap_svg(
        '  <g filter="url(#softShadow)">\n'
        f'    <path d="{four_way}" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.3)" stroke-width="0.9" stroke-linejoin="round"/>\n'
        f'    <path d="{four_way}" fill="url(#glassSheen)" '
        'stroke="url(#anodizedRim)" stroke-width="0.45" stroke-linejoin="round"/>\n'
        '    <circle cx="12" cy="12" r="2.7" fill="url(#emeraldGlow)"/>\n'
        '    <polygon points="12,10.1 13.9,12 12,13.9 10.1,12" fill="url(#emeraldGrad)" '
        'stroke="rgba(255,255,255,0.35)" stroke-width="0.4"/>\n'
        "  </g>"
    )


def svg_not_allowed() -> str:
    return wrap_svg(
        '  <g filter="url(#softShadow)">\n'
        '    <circle cx="12" cy="12" r="8.2" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.28)" stroke-width="0.95"/>\n'
        '    <circle cx="12" cy="12" r="7.8" fill="url(#glassSheen)"/>\n'
        '    <circle cx="12" cy="12" r="5.9" fill="none" stroke="url(#dangerGrad)" stroke-width="1.9"/>\n'
        '    <line x1="7.9" y1="7.9" x2="16.1" y2="16.1" stroke="url(#dangerGrad)" stroke-width="1.9" stroke-linecap="round"/>\n'
        '    <circle cx="12" cy="12" r="8.2" fill="none" stroke="url(#anodizedRim)" stroke-width="0.55"/>\n'
        "  </g>"
    )


def svg_help() -> str:
    # Arrow tip at (2, 2) + floating dark-glass emerald "?" badge bottom-right
    return wrap_svg(
        pointer_arrow_group(with_emerald_tail=False)
        + "\n"
        '  <g filter="url(#softShadow)">\n'
        '    <circle cx="16.8" cy="16.5" r="4.8" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.3)" stroke-width="0.85"/>\n'
        '    <circle cx="16.8" cy="16.5" r="4.4" fill="url(#glassSheen)"/>\n'
        '    <path d="M 15.3 15.1 C 15.3 14.0 16.05 13.4 16.9 13.4 C 17.8 13.4 18.45 14.0 18.45 14.75 '
        'C 18.45 15.6 17.4 15.95 16.85 16.55 L 16.85 17.2" '
        'fill="none" stroke="url(#emeraldGrad)" stroke-width="1.45" stroke-linecap="round"/>\n'
        '    <circle cx="16.85" cy="18.75" r="0.8" fill="#34D399"/>\n'
        "  </g>"
    )


def svg_resize_double(angle_deg: float) -> str:
    # Horizontal double arrow centered at (12, 12), rotated around (12, 12)
    h_arrow = (
        "M 2.6 12 L 6.8 8.3 L 6.8 10.65 L 17.2 10.65 L 17.2 8.3 L 21.4 12 "
        "L 17.2 15.7 L 17.2 13.35 L 6.8 13.35 L 6.8 15.7 Z"
    )
    return wrap_svg(
        f'  <g filter="url(#softShadow)" transform="rotate({angle_deg} 12 12)">\n'
        f'    <path d="{h_arrow}" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.3)" stroke-width="0.9" stroke-linejoin="round"/>\n'
        f'    <path d="{h_arrow}" fill="url(#glassSheen)" '
        'stroke="url(#anodizedRim)" stroke-width="0.45" stroke-linejoin="round"/>\n'
        '    <circle cx="12" cy="12" r="2.3" fill="url(#emeraldGlow)"/>\n'
        '    <rect x="10.8" y="10.8" width="2.4" height="2.4" rx="0.6" fill="url(#emeraldGrad)"/>\n'
        "  </g>"
    )


def svg_spinner_ring(cx: float, cy: float, r: float, stroke_w: float, angle_deg: float) -> str:
    # Smooth rotating Oksavi emerald comet ring over a dark-glass circular track.
    circ = 2.0 * math.pi * r
    arc_len = circ * 0.62
    gap_len = circ - arc_len
    return (
        f'  <g transform="translate({cx} {cy})">\n'
        f'    <circle cx="0" cy="0" r="{r + stroke_w * 0.65:.2f}" fill="url(#darkGlass)" '
        'stroke="rgba(255,255,255,0.28)" stroke-width="0.85"/>\n'
        f'    <circle cx="0" cy="0" r="{r + stroke_w * 0.55:.2f}" fill="url(#glassSheen)"/>\n'
        f'    <circle cx="0" cy="0" r="{r:.2f}" fill="none" '
        f'stroke="rgba(16,185,129,0.22)" stroke-width="{stroke_w:.2f}"/>\n'
        f'    <g transform="rotate({angle_deg:.1f})">\n'
        f'      <circle cx="0" cy="0" r="{r:.2f}" fill="none" stroke="url(#emeraldGrad)" '
        f'stroke-width="{stroke_w:.2f}" stroke-linecap="round" '
        f'stroke-dasharray="{arc_len:.2f} {gap_len:.2f}"/>\n'
        f'      <circle cx="{r:.2f}" cy="0" r="{stroke_w * 0.52:.2f}" fill="#A7F3D0"/>\n'
        "    </g>\n"
        "  </g>"
    )


def svg_wait(frame_idx: int) -> str:
    angle = (360.0 / ANIM_FRAMES) * frame_idx
    return wrap_svg(
        '  <g filter="url(#softShadow)">\n'
        + svg_spinner_ring(12.0, 12.0, 6.2, 2.1, angle)
        + "\n    <circle cx=\"12\" cy=\"12\" r=\"1.45\" fill=\"url(#emeraldGrad)\"/>\n"
        + "  </g>"
    )


def svg_progress(frame_idx: int) -> str:
    angle = (360.0 / ANIM_FRAMES) * frame_idx
    return wrap_svg(
        pointer_arrow_group(with_emerald_tail=False)
        + "\n"
        '  <g filter="url(#softShadow)">\n'
        + svg_spinner_ring(17.1, 17.1, 3.3, 1.35, angle)
        + "\n  </g>"
    )


def render_svg_to_png(svg_path: Path, png_path: Path, size: int) -> bytes:
    svg_bytes = svg_path.read_bytes()
    handle = Rsvg.Handle.new_from_data(svg_bytes)
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    ctx = cairo.Context(surface)
    ctx.set_operator(cairo.OPERATOR_CLEAR)
    ctx.paint()
    ctx.set_operator(cairo.OPERATOR_OVER)

    viewport = Rsvg.Rectangle()
    viewport.x = 0.0
    viewport.y = 0.0
    viewport.width = float(size)
    viewport.height = float(size)
    handle.render_document(ctx, viewport)
    surface.flush()

    png_path.parent.mkdir(parents=True, exist_ok=True)
    surface.write_to_png(str(png_path))
    return bytes(surface.get_data())


def scale_hotspot(hot_24: tuple[int, int], size: int) -> tuple[int, int]:
    return (
        max(0, min(size - 1, round(hot_24[0] * size / 24.0))),
        max(0, min(size - 1, round(hot_24[1] * size / 24.0))),
    )


def write_xcursor_file(out_path: Path, frames: list[tuple[int, int, int, int, bytes]]) -> None:
    """
    Writes a valid Xcursor v1.0 binary file.
    Each entry in `frames` is: (nominal_size, xhot, yhot, delay_ms, argb_premul_bytes)
    where `argb_premul_bytes` is Cairo ARGB32 little-endian data (size*size*4 bytes).
    """
    XCURSOR_MAGIC = b"Xcur"
    XCURSOR_FILE_HEADER_LEN = 16
    XCURSOR_FILE_VERSION = 0x00010000
    XCURSOR_IMAGE_TYPE = 0xFFFD0002
    XCURSOR_IMAGE_HEADER_LEN = 36
    XCURSOR_IMAGE_VERSION = 1

    ntoc = len(frames)
    header = struct.pack("<4sIII", XCURSOR_MAGIC, XCURSOR_FILE_HEADER_LEN, XCURSOR_FILE_VERSION, ntoc)

    toc_bytes = bytearray()
    chunks_bytes = bytearray()
    offset = XCURSOR_FILE_HEADER_LEN + 12 * ntoc

    for nominal_size, xhot, yhot, delay_ms, pixel_bytes in frames:
        expected = nominal_size * nominal_size * 4
        if len(pixel_bytes) != expected:
            raise ValueError(f"Pixel buffer size mismatch: {len(pixel_bytes)} != {expected}")
        toc_bytes.extend(struct.pack("<III", XCURSOR_IMAGE_TYPE, nominal_size, offset))
        chunk_hdr = struct.pack(
            "<IIIIIIIII",
            XCURSOR_IMAGE_HEADER_LEN,
            XCURSOR_IMAGE_TYPE,
            nominal_size,
            XCURSOR_IMAGE_VERSION,
            nominal_size,
            nominal_size,
            xhot,
            yhot,
            delay_ms,
        )
        chunks_bytes.extend(chunk_hdr)
        chunks_bytes.extend(pixel_bytes)
        offset += XCURSOR_IMAGE_HEADER_LEN + expected

    out_path.write_bytes(header + bytes(toc_bytes) + bytes(chunks_bytes))


def build_cursor(
    name: str,
    frame_svgs: list[tuple[str, str]],
    hot_24: tuple[int, int],
    delay_ms: int = 0,
) -> None:
    cfg_lines: list[str] = []
    xcur_frames: list[tuple[int, int, int, int, bytes]] = []

    for frame_id, svg_text in frame_svgs:
        svg_path = SVG_DIR / f"{frame_id}.svg"
        svg_path.write_text(svg_text, encoding="utf-8")
        for size in SIZES:
            png_path = PNG_DIR / str(size) / f"{frame_id}.png"
            pixel_bytes = render_svg_to_png(svg_path, png_path, size)
            hx, hy = scale_hotspot(hot_24, size)
            rel_png = png_path.relative_to(BASE_DIR)
            if delay_ms > 0:
                cfg_lines.append(f"{size} {hx} {hy} {rel_png} {delay_ms}")
            else:
                cfg_lines.append(f"{size} {hx} {hy} {rel_png}")
            xcur_frames.append((size, hx, hy, delay_ms, pixel_bytes))

    cfg_path = CFG_DIR / f"{name}.cursor"
    cfg_path.write_text("\n".join(cfg_lines) + "\n", encoding="utf-8")

    out_cursor = CUR_DIR / name
    xcursorgen_bin = shutil.which("xcursorgen")
    if xcursorgen_bin:
        subprocess.run(
            [xcursorgen_bin, "-p", str(BASE_DIR), str(cfg_path), str(out_cursor)],
            check=True,
        )
    else:
        write_xcursor_file(out_cursor, xcur_frames)


def main() -> None:
    if CUR_DIR.exists():
        shutil.rmtree(CUR_DIR)
    for d in (SVG_DIR, CFG_DIR, CUR_DIR):
        d.mkdir(parents=True, exist_ok=True)
    for s in SIZES:
        (PNG_DIR / str(s)).mkdir(parents=True, exist_ok=True)

    static_cursors: dict[str, tuple[str, tuple[int, int]]] = {
        "default": (svg_default(), (2, 2)),
        "pointer": (svg_pointer(), (9, 2)),
        "text": (svg_text(), (12, 12)),
        "crosshair": (svg_crosshair(), (12, 12)),
        "move": (svg_move(), (12, 12)),
        "grab": (svg_grab(closed=False), (12, 12)),
        "grabbing": (svg_grab(closed=True), (12, 12)),
        "not-allowed": (svg_not_allowed(), (12, 12)),
        "help": (svg_help(), (2, 2)),
        "ew-resize": (svg_resize_double(0.0), (12, 12)),
        "ns-resize": (svg_resize_double(90.0), (12, 12)),
        "nwse-resize": (svg_resize_double(45.0), (12, 12)),
        "nesw-resize": (svg_resize_double(-45.0), (12, 12)),
    }

    for name, (svg_body, hot_24) in static_cursors.items():
        build_cursor(name, [(name, svg_body)], hot_24, delay_ms=0)

    wait_frames = [(f"wait-{i:02d}", svg_wait(i)) for i in range(ANIM_FRAMES)]
    build_cursor("wait", wait_frames, (12, 12), delay_ms=ANIM_DELAY_MS)

    progress_frames = [(f"progress-{i:02d}", svg_progress(i)) for i in range(ANIM_FRAMES)]
    build_cursor("progress", progress_frames, (2, 2), delay_ms=ANIM_DELAY_MS)

    aliases: dict[str, str] = {
        # Basic / X11 / GTK / Qt / Wayland shape aliases
        "left_ptr": "default",
        "arrow": "default",
        "top_left_arrow": "default",
        "dnd-move": "default",
        "hand1": "pointer",
        "hand2": "pointer",
        "pointing_hand": "pointer",
        "xterm": "text",
        "ibeam": "text",
        "cross": "crosshair",
        "cross_reverse": "crosshair",
        "diamond_cross": "crosshair",
        "tcross": "crosshair",
        "fleur": "move",
        "all-scroll": "move",
        "all-resize": "move",
        "size_all": "move",
        "openhand": "grab",
        "closedhand": "grabbing",
        "dnd-none": "grabbing",
        "crossed_circle": "not-allowed",
        "no-drop": "not-allowed",
        "forbidden": "not-allowed",
        "circle": "not-allowed",
        "question_arrow": "help",
        "whats_this": "help",
        "left_ptr_help": "help",
        # Resize aliases
        "size_fdiag": "nwse-resize",
        "bd_double_arrow": "nwse-resize",
        "nw-resize": "nwse-resize",
        "se-resize": "nwse-resize",
        "top_left_corner": "nwse-resize",
        "bottom_right_corner": "nwse-resize",
        "size_bdiag": "nesw-resize",
        "fd_double_arrow": "nesw-resize",
        "ne-resize": "nesw-resize",
        "sw-resize": "nesw-resize",
        "top_right_corner": "nesw-resize",
        "bottom_left_corner": "nesw-resize",
        "sb_h_double_arrow": "ew-resize",
        "h_double_arrow": "ew-resize",
        "size_hor": "ew-resize",
        "col-resize": "ew-resize",
        "split_h": "ew-resize",
        "e-resize": "ew-resize",
        "w-resize": "ew-resize",
        "left_side": "ew-resize",
        "right_side": "ew-resize",
        "sb_v_double_arrow": "ns-resize",
        "v_double_arrow": "ns-resize",
        "size_ver": "ns-resize",
        "row-resize": "ns-resize",
        "split_v": "ns-resize",
        "n-resize": "ns-resize",
        "s-resize": "ns-resize",
        "top_side": "ns-resize",
        "bottom_side": "ns-resize",
        # Animated loading aliases
        "watch": "wait",
        "left_ptr_watch": "progress",
        "half-busy": "progress",
    }

    for alias, target in aliases.items():
        link_path = CUR_DIR / alias
        if link_path.exists() or link_path.is_symlink():
            link_path.unlink()
        link_path.symlink_to(target)

    (BASE_DIR / "index.theme").write_text(
        "[Icon Theme]\n"
        "Name=okay-cursors\n"
        "Comment=Premium Dark Glass & Anodized Metal cursors with Oksavi emerald accents for OKay DE\n"
        "Inherits=Adwaita,hicolor\n",
        encoding="utf-8",
    )
    (BASE_DIR / "cursor.theme").write_text(
        "[Icon Theme]\n"
        "Inherits=okay-cursors\n",
        encoding="utf-8",
    )

    print(
        f"Built {len(static_cursors) + 2} compiled Xcursor files "
        f"({ANIM_FRAMES} frames for wait & progress @ {ANIM_DELAY_MS}ms), "
        f"{len(aliases)} symlinks in {CUR_DIR}"
    )


if __name__ == "__main__":
    main()
