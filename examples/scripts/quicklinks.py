#!/usr/bin/env python3
"""
# @aerofi.schemaVersion 1
# @aerofi.title Quick Links
# @aerofi.mode gui
# @aerofi.icon 🔗
# @aerofi.packageName Productivity
# @aerofi.description Your quick links — type a URL to add a new one
# @aerofi.show_search true

Quick Links — a personal list of links stored in
~/.config/aerofi/quicklinks.json:

    {
      "Work": ["Docs: https://docs.example.com", "https://bare-url.com"],
      "Home": ["Router: https://192.168.0.1"]
    }

Inside aerofi:
  • Select a row and press Enter -> opens the URL in your browser
  • Type a URL (or "Title: URL") so nothing matches, press Enter ->
    the link is added to the "Custom" folder and the list refreshes

New links are stored as "Title: https://…" (or the bare URL when the
title is just the domain). Edit the JSON file to reorganize folders.
"""

import json
import subprocess
import sys
from pathlib import Path
from urllib.parse import urlparse

USER_FILE = Path.home() / ".config/aerofi/quicklinks.json"
CUSTOM_FOLDER = "Custom"


# ── aerofi helpers ───────────────────────────────────────────────────────

def send(line: str) -> None:
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def short_domain(url: str) -> str:
    host = urlparse(url).netloc
    return host[4:] if host.startswith("www.") else host


def emit(groups: list[tuple[str, list[tuple[str, str]]]], message: str) -> None:
    send("\0prompt\x1fSearch or type a URL to add…")
    send(f"\0message\x1f{message}")
    if not groups:
        send("Type a URL, e.g. https://example.com\0icon\x1f➕\0nonselectable\x1ftrue")
        send("or with a title: GitHub: https://github.com\0icon\x1f✍️\0nonselectable\x1ftrue")
    for folder, links in groups:
        send(f"{folder}\0icon\x1f📁\0info\x1f{len(links)}\0nonselectable\x1ftrue")
        for title, url in links:
            parts = [title]
            parts.append("\0icon\x1f🌐")
            parts.append(f"\0info\x1f{short_domain(url)}")
            parts.append(f"\0meta\x1f{folder} {title} {url}")
            send("".join(parts))
    send("\0flush")


# ── Storage ──────────────────────────────────────────────────────────────

def normalize_url(url: str) -> str | None:
    url = url.strip()
    if url and not url.startswith(("http://", "https://")):
        url = "https://" + url
    parsed = urlparse(url)
    if parsed.scheme not in ("http", "https") or not parsed.netloc:
        return None
    return url


def parse_entry(entry: str) -> tuple[str, str] | None:
    """Parse 'Title: https://…' or a bare URL into (title, url)."""
    text = entry.strip()
    if not text:
        return None
    title, url = "", text
    idx = text.find(":")
    if idx > 0 and text[idx + 1 : idx + 3] != "//":
        head, tail = text[:idx].strip(), text[idx + 1 :].strip()
        if head and tail:
            title, url = head, tail
    url = normalize_url(url)
    if url is None:
        return None
    return (title or short_domain(url), url)


def load_groups() -> list[tuple[str, list[tuple[str, str]]]]:
    try:
        data = json.loads(USER_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return []
    groups: list[tuple[str, list[tuple[str, str]]]] = []
    if isinstance(data, dict):
        for folder, entries in data.items():
            if not isinstance(entries, list):
                continue
            links = []
            for entry in entries:
                if isinstance(entry, str):
                    parsed = parse_entry(entry)
                    if parsed:
                        links.append(parsed)
            if links:
                groups.append((folder, links))
    return groups


def save_groups(groups: list[tuple[str, list[tuple[str, str]]]]) -> None:
    data: dict[str, list[str]] = {}
    for folder, links in groups:
        entries = []
        for title, url in links:
            if title == short_domain(url):
                entries.append(url)
            else:
                entries.append(f"{title}: {url}")
        data[folder] = entries
    USER_FILE.parent.mkdir(parents=True, exist_ok=True)
    USER_FILE.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def add_link(groups: list[tuple[str, list[tuple[str, str]]]], title: str, url: str) -> bool:
    for _, links in groups:
        if any(u == url for _, u in links):
            return False
    for folder, links in groups:
        if folder == CUSTOM_FOLDER:
            links.append((title, url))
            return True
    groups.append((CUSTOM_FOLDER, [(title, url)]))
    return True


def open_url(url: str) -> None:
    subprocess.Popen(
        ["open", url],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


# ── Event loop ───────────────────────────────────────────────────────────

def parse_event(line: str) -> tuple[str, dict[str, str]] | None:
    """Parse '\\0event\\x1f<type>\\x1fkey:value…' -> (type, fields)."""
    parts = line.lstrip("\x00").rstrip("\r\n").split("\x1f")
    if len(parts) < 2 or parts[0] != "event":
        return None
    fields: dict[str, str] = {}
    for part in parts[2:]:
        if ":" in part:
            key, value = part.split(":", 1)
            fields[key] = value
    return parts[1], fields


def main() -> None:
    groups = load_groups()
    emit(groups, "Select a link to open it — type a URL to add a new one")

    while True:
        line = sys.stdin.readline()
        if not line:
            return
        parsed = parse_event(line)
        if parsed is None:
            continue
        etype, fields = parsed
        if etype == "select":
            selected = fields.get("text", "")
            for _, links in groups:
                for title, url in links:
                    if title == selected:
                        open_url(url)
                        return
        elif etype == "custom":
            parsed = parse_entry(fields.get("text", ""))
            if parsed:
                title, url = parsed
                if add_link(groups, title, url):
                    save_groups(groups)
                    emit(groups, f"Added {title}")
                else:
                    emit(groups, f"{title} is already in your links")


if __name__ == "__main__":
    main()
