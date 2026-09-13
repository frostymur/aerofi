#!/usr/bin/env python3
"""
# @aerofi.schemaVersion 1
# @aerofi.title Theme Switcher
# @aerofi.mode gui
# @aerofi.icon 🎨
# @aerofi.packageName aerofi
# @aerofi.description Switch and preview themes live in aerofi
# @aerofi.show_search true
# @aerofi.columns 1
"""

import os
import re
import subprocess
import sys
from pathlib import Path

# Built-in themes known to aerofi
BUILTIN_THEMES = [
    {
        "slug": "default",
        "name": "Dark Transparent (Default)",
        "author": "aerofi",
        "accent": "#7aa2f7",
        "bg": "#1a1b26",
        "surface": "#24283b",
        "text": "#c0caf5",
        "desc": "Built-in Tokyo Night dark transparent palette",
    }
]


def get_config_dir() -> Path:
    xdg = os.environ.get("XDG_CONFIG_HOME")
    if xdg:
        return Path(xdg) / "aerofi"
    return Path.home() / ".config" / "aerofi"


def get_current_theme(config_file: Path) -> str:
    if not config_file.exists():
        return "default"
    try:
        content = config_file.read_text(encoding="utf-8")
        match = re.search(r'^\s*theme\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if match:
            return match.group(1).strip()
    except Exception:
        pass
    return "default"


def set_current_theme(config_file: Path, theme_slug: str) -> bool:
    try:
        config_dir = config_file.parent
        config_dir.mkdir(parents=True, exist_ok=True)

        if not config_file.exists():
            config_file.write_text(f'theme = "{theme_slug}"\n', encoding="utf-8")
            return True

        content = config_file.read_text(encoding="utf-8")
        pattern = r'^(\s*theme\s*=\s*)"[^"]+"'
        if re.search(pattern, content, re.MULTILINE):
            updated = re.sub(pattern, rf'\g<1>"{theme_slug}"', content, flags=re.MULTILINE)
        else:
            updated = f'theme = "{theme_slug}"\n' + content

        config_file.write_text(updated, encoding="utf-8")
        return True
    except Exception as err:
        sys.stderr.write(f"aerofi theme-switcher: error writing config: {err}\n")
        return False


def parse_theme_file(path: Path) -> dict:
    slug = path.stem
    res = {
        "slug": slug,
        "name": slug.replace("-", " ").title(),
        "author": "aerofi",
        "accent": "#7aa2f7",
        "bg": "#1a1b26",
        "surface": "#24283b",
        "text": "#c0caf5",
        "desc": f"Theme from {path.name}",
    }

    try:
        content = path.read_text(encoding="utf-8")
        name_m = re.search(r'^\s*name\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if name_m:
            res["name"] = name_m.group(1).strip()

        author_m = re.search(r'^\s*author\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if author_m:
            res["author"] = author_m.group(1).strip()

        # Parse [colors] table for alias resolution
        palette = {}
        colors_section = False
        for line in content.splitlines():
            line_str = line.strip()
            if line_str.startswith("[") and line_str.endswith("]"):
                colors_section = (line_str.strip("[]").strip() == "colors")
                continue
            if colors_section and "=" in line_str:
                parts = line_str.split("=", 1)
                k = parts[0].strip()
                v = parts[1].strip().strip('"').strip("'")
                palette[k] = v

        # Extract accent color
        accent_m = re.search(r'^\s*accent\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if accent_m:
            raw = accent_m.group(1).strip()
            res["accent"] = palette.get(raw.lstrip("$"), raw)

        # Extract background color
        bg_m = re.search(r'^\s*bg\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if bg_m:
            raw = bg_m.group(1).strip()
            res["bg"] = palette.get(raw.lstrip("$"), raw)

        # Extract text color
        text_m = re.search(r'^\s*text\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if text_m:
            raw = text_m.group(1).strip()
            res["text"] = palette.get(raw.lstrip("$"), raw)

        # Extract surface color
        surf_m = re.search(r'^\s*surface\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if surf_m:
            raw = surf_m.group(1).strip()
            res["surface"] = palette.get(raw.lstrip("$"), raw)

        # If still aliases or defaults, check palette directly
        for k in ("accent", "bg", "surface", "text"):
            if res[k].startswith("$"):
                var = res[k].lstrip("$")
                if var in palette:
                    res[k] = palette[var]
            elif k in palette and res[k].startswith("$"):
                res[k] = palette[k]
    except Exception:
        pass

    # Normalize hex colors (strip alpha if 8-digit)
    for k in ("accent", "bg", "surface", "text"):
        val = res[k]
        if val.startswith("#") and len(val) == 9:
            res[k] = val[:7]

    return res


def discover_themes(config_dir: Path) -> list[dict]:
    themes = {}
    for bt in BUILTIN_THEMES:
        themes[bt["slug"]] = bt

    themes_dir = config_dir / "themes"
    if themes_dir.is_dir():
        for p in sorted(themes_dir.glob("*.toml")):
            t = parse_theme_file(p)
            themes[t["slug"]] = t

    # Also check repo examples if local
    repo_examples = Path(__file__).resolve().parent.parent / "themes"
    if repo_examples.is_dir():
        for p in sorted(repo_examples.glob("*.toml")):
            if p.stem not in themes:
                t = parse_theme_file(p)
                themes[t["slug"]] = t

    return list(themes.values())


def render_pango_row(theme: dict, is_active: bool) -> str:
    accent = theme.get("accent", "#7aa2f7")
    text_color = theme.get("text", "#c0caf5")
    name = theme.get("name", theme["slug"])
    slug = theme["slug"]

    # Visual color swatch preview
    swatch = f'<span foreground="{accent}">■</span> <span foreground="{theme.get("bg", "#222222")}">■</span>'
    title_span = f'<span foreground="{text_color}" weight="bold">{name}</span>'
    slug_span = f'<span foreground="#666666">({slug})</span>'

    if is_active:
        status = '<span foreground="#9ece6a" weight="bold">✓ Active</span>'
    else:
        status = f'<span foreground="#888888">by {theme.get("author", "aerofi")}</span>'

    row_text = f"{swatch}  {title_span} {slug_span}"
    info_field = f"\0info\x1f{status}"
    meta_field = f"\0meta\x1f{name} {slug} {theme.get('author', '')} {'active current' if is_active else ''}"
    icon_field = "\0icon\x1femoji:🎨"

    return f"{row_text}{icon_field}{info_field}{meta_field}"


def notify_user(title: str, msg: str):
    try:
        script = f'display notification "{msg}" with title "{title}" sound name "Glass"'
        subprocess.run(["osascript", "-e", script], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception:
        pass


def main():
    config_dir = get_config_dir()
    config_file = config_dir / "config.toml"
    current_theme = get_current_theme(config_file)
    themes = discover_themes(config_dir)

    # Sort so active theme is at the top, followed by alphabetically sorted themes
    themes.sort(key=lambda t: (0 if t["slug"] == current_theme else 1, t["name"].lower()))

    # Emit GUI initial frame
    sys.stdout.write("\0prompt\x1fSearch & select aerofi theme…\n")
    sys.stdout.write("\0markup-rows\x1ftrue\n")
    sys.stdout.write("\0message\x1fPress Enter to activate theme • Press Cmd+R in aerofi to reload\n")

    for i, t in enumerate(themes):
        is_active = (t["slug"] == current_theme)
        row = render_pango_row(t, is_active)
        sys.stdout.write(f"{row}\n")

    sys.stdout.write("\0flush\n")
    sys.stdout.flush()

    # Listen for selection event on stdin
    while True:
        line = sys.stdin.readline()
        if not line:
            break

        line = line.strip()
        if not line:
            continue

        # Format: \0event\x1fselect\x1fkey=enter\x1findex=0\x1f...
        # or plain text if stdin receives return
        if "select" in line or line.startswith("\0event"):
            # Extract index
            index = None
            for part in line.split("\x1f"):
                if part.startswith("index="):
                    try:
                        index = int(part.split("=", 1)[1])
                    except ValueError:
                        pass

            if index is not None and 0 <= index < len(themes):
                selected = themes[index]
            else:
                # Default to highlighted or first
                selected = themes[0]

            # Apply theme
            selected_slug = selected["slug"]
            selected_name = selected["name"]
            if set_current_theme(config_file, selected_slug):
                notify_user(
                    "aerofi Theme Switcher",
                    f"Theme changed to '{selected_name}'. Reload with Cmd+R."
                )
            break


if __name__ == "__main__":
    main()
