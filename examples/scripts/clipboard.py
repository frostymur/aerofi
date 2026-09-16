#!/usr/bin/env python3
"""
# @aerofi.schemaVersion 1
# @aerofi.title Clipboard History
# @aerofi.mode gui
# @aerofi.icon 
# @aerofi.packageName System
# @aerofi.description Search and manage your clipboard history
# @aerofi.show_search true
# @aerofi.columns 1
# @aerofi.layout list

Clipboard history manager for aerofi's interactive gui mode.
Powered by the clipy daemon (https://crates.io/crates/clipy) with a
pbcopy/pbpaste fallback. Enter copies, Tab multi-selects, Ctrl+D deletes.
Images are captured from the pasteboard on demand via the clippy suite
(https://github.com/neilberkman/clippy: `pasty` extracts, `clippy` copies
back) and shown with thumbnail previews.
"""

from __future__ import annotations

import hashlib
import html
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import time

HINT = "↵ Copy · ⇥ Multi-select · ⌃D Delete · images auto-saved"

IC_CLIPBOARD = "\uF0EA"
IC_GLOBE = "\uF0AC"
IC_PAINT = "\uF1FC"
IC_ENVELOPE = "\uF0E0"
IC_CUBE = "\uF1B2"
IC_TERMINAL = "\uF120"
IC_FILE = "\uF15B"
IC_EDIT = "\uF044"
IC_IMAGE = "\uF03E"
IC_WARNING = "\uF071"

MAX_IMAGES = 50
IMG_LIST_CAP = 20
CAPTURE_INTERVAL = 2.0


def clean(value: str) -> str:
    """Make a string safe for a single protocol line."""
    return value.replace("\x00", " ").replace("\x1f", " ").replace("\r", " ").replace("\n", " ")


def pbpaste() -> str:
    try:
        res = subprocess.run(["pbpaste"], capture_output=True, text=True, timeout=1.0)
        return res.stdout
    except Exception:
        return ""


def pbcopy(text: str) -> None:
    try:
        proc = subprocess.Popen(["pbcopy"], stdin=subprocess.PIPE)
        proc.communicate(text.encode("utf-8"))
    except Exception:
        pass


def clipy_bin() -> str | None:
    for path in (shutil.which("clipy"), os.path.expanduser("~/.cargo/bin/clipy")):
        if path and os.access(path, os.X_OK):
            return path
    return None


def ensure_daemon(binary: str) -> None:
    try:
        if subprocess.run([binary, "status"], capture_output=True, timeout=1.0).returncode == 0:
            return
        subprocess.Popen(
            [binary, "watch"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        for _ in range(10):
            time.sleep(0.05)
            if subprocess.run([binary, "status"], capture_output=True, timeout=0.5).returncode == 0:
                return
    except Exception:
        pass


def load_from_db() -> list[dict] | None:
    db = os.path.expanduser("~/Library/Application Support/clipy-rust/history.db")
    if not os.path.isfile(db):
        return None
    try:
        conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=1.0)
        rows = conn.execute(
            "SELECT id, content, updated_at FROM history ORDER BY updated_at DESC LIMIT 200"
        ).fetchall()
        conn.close()
    except Exception:
        return None
    return [
        {"id": r[0], "content": r[1], "updated_at": r[2]}
        for r in rows
        if isinstance(r[1], str)
    ]


def load_from_cli(binary: str) -> list[dict]:
    try:
        res = subprocess.run(
            [binary, "list", "-l", "60"], capture_output=True, text=True, timeout=2.0
        )
    except Exception:
        return []
    if res.returncode != 0:
        return []
    entries = []
    for line in res.stdout.splitlines():
        parts = line.split(None, 3)
        # A real line is "<8-hex id>  <N>m ago  <content>"; anything else
        # (e.g. the "(no clipboard history yet)" placeholder) is skipped.
        if len(parts) == 4 and re.fullmatch(r"[0-9a-f]{8,64}", parts[0]):
            entries.append({"id": parts[0], "content": parts[3], "updated_at": time.time()})
    return entries


def relative_time(ts: float) -> str:
    diff = max(0, int(time.time() - ts))
    if diff < 10:
        return "now"
    if diff < 60:
        return f"{diff}s ago"
    mins = diff // 60
    if mins < 60:
        return f"{mins}m ago"
    hours = mins // 60
    if hours < 24:
        return f"{hours}h ago"
    days = hours // 24
    if days == 1:
        return "yesterday"
    if days < 7:
        return f"{days}d ago"
    return time.strftime("%b %d", time.localtime(ts))


# ---------------------------------------------------------------------------
# Image history
#
# Text entries come from the clipy daemon; images are captured on demand from
# the pasteboard when the switcher is (re)drawn, using the clippy suite
# (https://github.com/neilberkman/clippy, `brew install clippy`):
#
#   pasty --inspect   show clipboard types (image detection)
#   pasty <path>      extract the image to a file (TIFF -> PNG)
#   clippy <path>     copy the file to the clipboard (paste-back)
#
# Without clippy/pasty the switcher silently degrades to text-only.
# ---------------------------------------------------------------------------

IMAGE_TYPES = ("public.png", "public.tiff", "com.apple.tiff", "public.jpeg")


def img_dir() -> str:
    return os.path.join(os.path.expanduser("~"), ".config", "aerofi", "clipboard-history")


def pasty_bin() -> str | None:
    return shutil.which("pasty")


def clippy_bin() -> str | None:
    return shutil.which("clippy")


def clipboard_has_image() -> bool:
    pbin = pasty_bin()
    if not pbin:
        return False
    try:
        res = subprocess.run([pbin, "--inspect"], capture_output=True, text=True, timeout=2)
    except Exception:
        return False
    return any(t in res.stdout for t in IMAGE_TYPES)


def sniff_image(path: str) -> str | None:
    """Return "png" or "jpg" from the file magic, else None."""
    try:
        with open(path, "rb") as fh:
            head = fh.read(8)
    except Exception:
        return None
    if head[:8] == b"\x89PNG\r\n\x1a\n":
        return "png"
    if head[:3] == b"\xff\xd8\xff":
        return "jpg"
    return None


def paste_image(path: str) -> bool:
    """Put the image file back on the clipboard (as a file reference)."""
    cbin = clippy_bin()
    if not cbin:
        return False
    try:
        res = subprocess.run([cbin, path], capture_output=True, timeout=5)
        return res.returncode == 0
    except Exception:
        return False


def manifest_path() -> str:
    return os.path.join(img_dir(), "manifest.json")


def load_manifest() -> dict:
    try:
        with open(manifest_path(), "r", encoding="utf-8") as fh:
            data = json.load(fh)
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def save_manifest(manifest: dict) -> None:
    try:
        os.makedirs(img_dir(), exist_ok=True)
        tmp = manifest_path() + ".tmp"
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(manifest, fh)
        os.replace(tmp, manifest_path())
    except Exception:
        pass


def png_dimensions(path: str):
    try:
        with open(path, "rb") as fh:
            head = fh.read(24)
        if head[:8] == b"\x89PNG\r\n\x1a\n" and len(head) >= 24:
            return (
                int.from_bytes(head[16:20], "big"),
                int.from_bytes(head[20:24], "big"),
            )
    except Exception:
        pass
    return None


def prune_images(manifest: dict) -> None:
    if len(manifest) <= MAX_IMAGES:
        return
    oldest_first = sorted(manifest.items(), key=lambda kv: kv[1].get("ts", 0))
    for digest, entry in oldest_first[: len(manifest) - MAX_IMAGES]:
        manifest.pop(digest, None)
        try:
            os.unlink(entry["path"])
        except Exception:
            pass


def capture_image() -> None:
    """Save the current pasteboard image (if any) into the image history."""
    pbin = pasty_bin()
    if not pbin or not clipboard_has_image():
        return
    os.makedirs(img_dir(), exist_ok=True)
    path = os.path.join(img_dir(), f"{int(time.time() * 1000)}.png")
    try:
        subprocess.run([pbin, path], capture_output=True, timeout=10)
    except Exception:
        return
    kind = sniff_image(path)
    if kind is None:
        try:
            os.unlink(path)
        except Exception:
            pass
        return
    if kind == "jpg":
        final = path[: -len(".png")] + ".jpg"
        os.replace(path, final)
        path = final
    with open(path, "rb") as fh:
        digest = hashlib.sha256(fh.read()).hexdigest()
    manifest = load_manifest()
    if digest in manifest:
        try:
            os.unlink(path)
        except Exception:
            pass
        return
    size = png_dimensions(path)
    manifest[digest] = {
        "path": path,
        "ts": time.time(),
        "w": size[0] if size else None,
        "h": size[1] if size else None,
    }
    prune_images(manifest)
    save_manifest(manifest)


def render_image_entry(digest: str, entry: dict) -> str:
    w, h = entry.get("w"), entry.get("h")
    size = f"{w}×{h} " if w and h else ""
    return (
        f"{IC_IMAGE} <b>{size}image</b>\0id\x1fimg:{digest[:8]}"
        f"\0icon\x1f{entry['path']}\0info\x1f{relative_time(entry.get('ts', time.time()))}"
        f"\0meta\x1fcopied image {size}".rstrip()
    )


def describe(content: str) -> tuple[str, str]:
    """Return (icon, pango text) for an entry, auto-detecting its type."""
    raw = content.strip()
    lines = raw.splitlines()
    first = html.escape(lines[0].strip()) if lines else ""

    if re.fullmatch(r"https?://\S+", raw, re.IGNORECASE):
        m = re.match(r"^https?://([^/]+)(/.*)?$", raw, re.IGNORECASE)
        if m:
            domain = html.escape(m.group(1))
            path = m.group(2) or ""
            if len(path) > 48:
                path = html.escape(path[:48]) + "…"
            else:
                path = html.escape(path)
            return IC_GLOBE, f"<b>{domain}</b>{path}"
        return IC_GLOBE, f"<b>{first[:70]}</b>"

    if re.fullmatch(r"#[0-9a-fA-F]{3,8}", raw):
        return IC_PAINT, f'<span foreground="{raw}">■</span> <b>{raw}</b>'

    if re.fullmatch(r"[a-zA-Z0-9_.+-]+@[a-zA-Z0-9-]+\.[a-zA-Z0-9-.]+", raw):
        return IC_ENVELOPE, f"<b>{first}</b>"

    if raw[:1] in "{[":
        try:
            parsed = json.loads(raw)
        except Exception:
            pass
        else:
            compact = html.escape(json.dumps(parsed, separators=(", ", ":")))
            if len(compact) > 64:
                compact = compact[:64] + "…"
            return IC_CUBE, compact

    keywords = (
        "def ", "fn ", "pub fn", "import ", "from ", "const ", "let ", "var ",
        "function", "class ", "struct ", "impl ", "interface ", "type ",
        "select ", "insert ", "update ", "delete ", "create table",
        "curl ", "git ", "cargo ", "npm ", "pnpm ", "bun ", "yarn ",
        "docker ", "kubectl ", "brew ", "chmod ", "chown ", "ssh ",
        "grep ", "find ", "echo ",
    )
    is_code = raw[:64].lower().startswith(keywords)
    if not is_code and len(lines) > 1:
        sample = raw[:200]
        is_code = "{" in sample and any(tok in sample for tok in ("=>", ";", ":", "("))

    if is_code:
        icon = IC_TERMINAL
    elif len(lines) > 1:
        icon = IC_FILE
    else:
        icon = IC_EDIT

    if len(lines) > 1:
        second = html.escape(lines[1].strip()[:36])
        if len(lines[1].strip()) > 36:
            second += "…"
        return icon, f"<b>{first[:56]}</b> <i>↵ {second}</i>"
    return icon, f"{first[:80]}"


def render_entry(entry: dict) -> str:
    icon, text = describe(entry["content"])
    info = relative_time(entry["updated_at"])
    meta = clean(entry["content"])[:1000]
    return (
        f"{text}\0id\x1f{entry['id']}\0icon\x1f{icon}"
        f"\0info\x1f{info}\0meta\x1f{meta}"
    )


def render_current() -> str:
    cur = pbpaste().strip()
    icon, text = describe(cur)
    return f"{text}\0id\x1fcurrent\0icon\x1f{icon}\0info\x1fnow\0meta\x1f{clean(cur)[:1000]}"


def emit(rows: list[str], message: str | None = None, selectable: bool = True) -> None:
    out = ["\0prompt\x1fSearch clipboard…", "\0markup-rows\x1ftrue"]
    if selectable:
        out.append("\0multi-select\x1ftrue")
    out.append(f"\0message\x1f{clean(message or HINT)}")
    out.extend(rows)
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
    binary = clipy_bin()
    if binary:
        ensure_daemon(binary)
    last_capture = 0.0

    def fetch() -> list[dict]:
        if binary:
            entries = load_from_db() or load_from_cli(binary)
            if entries:
                return entries
        return []

    entries = fetch()
    by_id = {e["id"]: e["content"] for e in entries}

    def lookup(rid: str) -> str | None:
        if rid in by_id:
            return by_id[rid]
        for key, value in by_id.items():
            if key.startswith(rid) or rid.startswith(key):
                return value
        return None

    def image_entries() -> list[tuple[str, dict]]:
        manifest = load_manifest()
        return sorted(
            manifest.items(), key=lambda kv: kv[1].get("ts", 0), reverse=True
        )[:IMG_LIST_CAP]

    def frame(message: str | None = None) -> None:
        nonlocal last_capture
        now = time.time()
        if now - last_capture >= CAPTURE_INTERVAL:
            last_capture = now
            capture_image()
        img_rows = [render_image_entry(d, e) for d, e in image_entries()]

        text_rows: list[str] | None = None
        if entries:
            text_rows = [render_entry(e) for e in entries]
        elif binary:
            if pbpaste().strip():
                text_rows = [render_current()]
        if text_rows is not None:
            emit(img_rows + text_rows, message)
            return
        if img_rows:
            emit(img_rows, message or "No text history — images only")
            return
        if binary:
            emit(
                [f"Clipboard is empty\0nonselectable\x1ftrue\0icon\x1f{IC_CLIPBOARD}"],
                "History is empty",
            )
            return
        rows = [
            f"<b>clipy is not installed</b>\0nonselectable\x1ftrue\0icon\x1f{IC_WARNING}\0info\x1fmissing dependency",
            f"<b>cargo install clipy</b>\0id\x1finstall_cmd\0icon\x1f{IC_CUBE}\0info\x1fEnter copies command",
        ]
        if pbpaste().strip():
            rows.append(render_current())
        emit(rows, message or "clipy is not installed — install it to keep history")

    frame()

    while True:
        raw = sys.stdin.buffer.readline()
        if not raw:
            break
        event = parse_event(raw.decode("utf-8", "replace"))
        kind = event.get("type")

        if kind == "select":
            ids = [i.strip() for i in event.get("ids", "").split(",") if i.strip()]
            pieces = []
            image_paths = []
            for rid in ids:
                if rid.startswith("img:"):
                    short = rid[len("img:"):]
                    entry = next(
                        (e for d, e in load_manifest().items() if d.startswith(short)), None
                    )
                    if entry:
                        image_paths.append(entry["path"])
                    continue
                if rid == "current":
                    cur = pbpaste()
                    if cur:
                        pieces.append(cur)
                    continue
                if rid == "install_cmd":
                    pbcopy("cargo install clipy")
                    break
                content = lookup(rid)
                if content is None and binary:
                    try:
                        res = subprocess.run(
                            [binary, "show", rid], capture_output=True, text=True, timeout=1.0
                        )
                        # clipy prints the entry header to stderr; stdout
                        # carries the content verbatim.
                        if res.returncode == 0:
                            content = res.stdout.rstrip("\n")
                    except Exception:
                        pass
                if content is not None:
                    pieces.append(content)
            # Single image selected on its own -> paste the image back.
            # In multi-selects images are skipped (they can't be joined).
            if len(ids) == 1 and image_paths and not pieces:
                paste_image(image_paths[0])
            elif pieces:
                if len(pieces) == 1:
                    pbcopy(pieces[0])
                else:
                    pbcopy("\n".join(p.rstrip("\n") for p in pieces))
            elif image_paths:
                paste_image(image_paths[0])
            break

        if kind == "custom":
            if event.get("text"):
                pbcopy(event["text"])
            break

        if kind == "action" and event.get("key") == "ctrl+d":
            rid = event.get("id", "")
            if rid.startswith("img:"):
                short = rid[len("img:"):]
                manifest = load_manifest()
                digest = next((d for d in manifest if d.startswith(short)), None)
                if digest:
                    entry = manifest.pop(digest)
                    try:
                        os.unlink(entry["path"])
                    except Exception:
                        pass
                    save_manifest(manifest)
                frame("Entry removed")
                continue
            if binary and rid and rid != "current":
                try:
                    subprocess.run([binary, "delete", rid], capture_output=True, timeout=1.5)
                except Exception:
                    pass
                entries = fetch()
                by_id = {e["id"]: e["content"] for e in entries}
                frame("Entry removed")
            continue

        # Any other event (context keys, unknown): keep the frame alive.
        frame()


if __name__ == "__main__":
    main()
