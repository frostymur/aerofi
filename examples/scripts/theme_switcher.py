#!/usr/bin/env python3
"""
# @aerofi.schemaVersion 1
# @aerofi.title Theme Switcher
# @aerofi.mode gui
# @aerofi.icon 
# @aerofi.packageName aerofi
# @aerofi.description Switch the active aerofi theme
# @aerofi.show_search true
# @aerofi.columns 1
# @aerofi.preset list

Interactive theme switcher for aerofi's gui mode. Scans
~/.config/aerofi/themes/*.toml plus the built-in default, renders each
palette as colour swatches, and live-reloads the launcher on selection.
"""

import html
import os
import re
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11 (e.g. the macOS system 3.9)
    tomllib = None

BUILTIN = {
    "slug": "default",
    "name": "Dark Transparent",
    "colors": {
        "bg": "#1a1b26",
        "surface": "#24283b",
        "text": "#c0caf5",
        "accent": "#7aa2f7",
    },
}

FALLBACKS = {
    "bg": "#1a1b26",
    "surface": "#24283b",
    "text": "#c0caf5",
    "accent": "#7aa2f7",
}


def _toml_load(path: Path) -> dict:
    """Load a TOML file; fall back to a minimal parser on Python < 3.11."""
    if tomllib is not None:
        with open(path, "rb") as fh:
            return tomllib.load(fh)
    data: dict = {}
    current: dict = data
    with open(path, "r", encoding="utf-8") as fh:
        for raw in fh:
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            if line.startswith("[") and line.endswith("]"):
                current = data.setdefault(line[1:-1].strip(), {})
                continue
            if "=" not in line:
                continue
            key, _, value = line.partition("=")
            key = key.strip().strip('"')
            value = value.strip()
            if value and value[0] in "\"'":
                quote = value[0]
                end = value.find(quote, 1)
                value = value[1:end] if end != -1 else value[1:]
            else:
                value = value.split("#", 1)[0].strip()
            current[key] = value
    return data


def config_dir() -> Path:
    xdg = os.environ.get("XDG_CONFIG_HOME")
    base = Path(xdg) if xdg else Path.home() / ".config"
    return base / "aerofi"


def current_theme(config_file: Path) -> str:
    try:
        cfg = _toml_load(config_file)
        theme = cfg.get("theme")
        return str(theme) if isinstance(theme, str) and theme else "default"
    except Exception:
        return "default"


def set_theme(config_file: Path, slug: str) -> bool:
    try:
        config_file.parent.mkdir(parents=True, exist_ok=True)
        if config_file.exists():
            content = config_file.read_text(encoding="utf-8")
        else:
            content = ""
        pattern = r'^\s*theme\s*=\s*"[^"]*"'
        if re.search(pattern, content, re.MULTILINE):
            content = re.sub(pattern, f'theme = "{slug}"', content, flags=re.MULTILINE)
        else:
            content = f'theme = "{slug}"\n' + content
        config_file.write_text(content, encoding="utf-8")
        return True
    except Exception:
        return False


def resolve(value: str, palette: dict) -> str:
    seen = set()
    while value.startswith("$") and value[1:] in palette and value not in seen:
        seen.add(value)
        value = palette[value[1:]]
    return value


def parse_theme_file(path: Path) -> dict:
    theme = {
        "slug": path.stem,
        "name": path.stem.replace("-", " ").replace("_", " ").title(),
        "colors": dict(FALLBACKS),
    }
    try:
        data = _toml_load(path)
    except Exception:
        return theme

    name = data.get("name")
    if isinstance(name, str) and name.strip():
        theme["name"] = name.strip()

    palette = data.get("colors")
    if not isinstance(palette, dict):
        palette = {}
    for key in ("bg", "surface", "text", "accent"):
        raw = palette.get(key)
        if isinstance(raw, str) and raw.strip():
            theme["colors"][key] = resolve(raw.strip(), palette)
    return theme


def discover(config: Path) -> list[dict]:
    themes = [dict(BUILTIN, colors=dict(BUILTIN["colors"]))]
    seen = {BUILTIN["slug"]}
    themes_dir = config / "themes"
    if themes_dir.is_dir():
        for path in sorted(themes_dir.glob("*.toml")):
            if path.stem in seen:
                continue
            theme = parse_theme_file(path)
            if theme["slug"] not in seen:
                themes.append(theme)
                seen.add(theme["slug"])
    return themes


def swatches(theme: dict) -> str:
    parts = []
    for key in ("bg", "surface", "text", "accent"):
        color = theme["colors"].get(key, FALLBACKS[key])
        if not re.fullmatch(r"#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?", color):
            color = FALLBACKS[key]
        parts.append(f'<span foreground="{color}">■</span>')
    return "".join(parts)


def clean(value: str) -> str:
    return value.replace("\x00", " ").replace("\x1f", " ").replace("\r", " ").replace("\n", " ")


def emit_frame(themes: list[dict]) -> None:
    out = [
        "\0prompt\x1fSearch themes…",
        "\0no-custom\x1ftrue",
        "\0markup-rows\x1ftrue",
        "\0message\x1f↵ Apply theme · instant reload",
    ]
    for theme in themes:
        name = html.escape(theme["name"])
        slug = theme["slug"]
        fields = f"{name}\0id\x1f{slug}\0info\x1f{swatches(theme)}\0meta\x1f{clean(theme['name'] + ' ' + slug + ' theme')}"
        out.append(fields)
    out.append("\0flush")
    sys.stdout.write("\n".join(out) + "\n")
    sys.stdout.flush()


def parse_event(line: str) -> dict:
    fields = line.lstrip("\x00").rstrip("\r\n").split("\x1f")
    if len(fields) < 2 or fields[0] != "event":
        return {}
    event = {"type": fields[1]}
    for field in fields[2:]:
        if ":" in field:
            key, value = field.split(":", 1)
            event[key] = value
    return event


def main() -> None:
    config = config_dir()
    config_file = config / "config.toml"
    active = current_theme(config_file)
    themes = discover(config)
    themes.sort(key=lambda t: (t["slug"] != active, t["name"].lower()))

    emit_frame(themes)

    by_slug = {t["slug"]: t for t in themes}

    while True:
        raw = sys.stdin.buffer.readline()
        if not raw:
            break
        event = parse_event(raw.decode("utf-8", "replace"))
        kind = event.get("type")

        if kind == "select":
            slug = event.get("id", "")
            theme = by_slug.get(slug)
            if theme is None:
                emit_frame(themes)
                continue
            if theme["slug"] == active:
                emit_frame(themes)
                continue
            if set_theme(config_file, theme["slug"]):
                sys.stdout.write("\0reload\x1ftrue\n\0flush\n")
                sys.stdout.flush()
            break

        # Any other event: keep the frame alive.
        emit_frame(themes)


if __name__ == "__main__":
    main()
