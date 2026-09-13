#!/usr/bin/env python3
"""
aerofi Glamorous Clipboard Manager Script
Powered by clipy CLI (https://crates.io/crates/clipy) and aerofi GUI mode with Pango markup.

Features:
- Live clipboard history powered by the clipy daemon and SQLite database.
- Rich Pango formatting with auto-detection of URLs, Hex Colors, Code, JSON, Emails, and Multiline text.
- Full-text fuzzy search via aerofi meta tags.
- Multi-selection support (Tab) to copy multiple snippets combined.
- Delete action support (Ctrl+D) to remove entries from history.
"""

# @raycast.schemaVersion 1
# @raycast.title Clipboard History
# @raycast.mode gui
# @raycast.packageName System
# @raycast.icon 📋
# @raycast.description Search, preview, and manage clipboard history with clipy
# @aerofi.show_search true
# @aerofi.columns 1

import html
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import time


def find_clipy_binary():
    """Find the path to the clipy executable."""
    candidates = [
        shutil.which("clipy"),
        os.path.expanduser("~/.cargo/bin/clipy"),
        "/usr/local/bin/clipy",
        "/opt/homebrew/bin/clipy",
    ]
    for path in candidates:
        if path and os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    return None


def ensure_clipy_daemon(clipy_bin):
    """Ensure the clipy watch daemon is running in the background."""
    if not clipy_bin:
        return False

    try:
        res = subprocess.run([clipy_bin, "status"], capture_output=True, timeout=1.0)
        if res.returncode == 0:
            return True
    except Exception:
        pass

    try:
        subprocess.Popen(
            [clipy_bin, "watch"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        for _ in range(8):
            time.sleep(0.05)
            res = subprocess.run([clipy_bin, "status"], capture_output=True, timeout=0.5)
            if res.returncode == 0:
                return True
    except Exception:
        pass

    return False


def get_history_db_path():
    """Return the path to clipy's SQLite database."""
    return os.path.expanduser("~/Library/Application Support/clipy-rust/history.db")


def load_entries_from_db(db_path, limit=100):
    """Load recent history entries directly from clipy SQLite database."""
    if not os.path.isfile(db_path):
        return None

    try:
        uri = f"file:{db_path}?mode=ro"
        conn = sqlite3.connect(uri, uri=True, timeout=1.0)
        c = conn.cursor()
        c.execute(
            "SELECT id, content, created_at, updated_at FROM history ORDER BY updated_at DESC LIMIT ?",
            (limit,),
        )
        rows = c.fetchall()
        conn.close()
        entries = []
        for r_id, content, created_at, updated_at in rows:
            entries.append({
                "id": r_id,
                "content": content,
                "created_at": created_at,
                "updated_at": updated_at,
            })
        return entries
    except Exception:
        return None


def load_entries_from_cli(clipy_bin, limit=60):
    """Fallback: load entries by invoking `clipy list`."""
    if not clipy_bin:
        return []

    try:
        res = subprocess.run(
            [clipy_bin, "list", "-l", str(limit)],
            capture_output=True,
            text=True,
            timeout=2.0,
        )
        if res.returncode != 0:
            return []

        entries = []
        for line in res.stdout.splitlines():
            line = line.strip()
            if not line:
                continue
            parts = line.split(None, 3)
            if len(parts) >= 4:
                item_id = parts[0]
                time_ago = f"{parts[1]} {parts[2]}"
                content = parts[3]
                entries.append({
                    "id": item_id,
                    "content": content,
                    "created_at": time.time(),
                    "updated_at": time.time(),
                    "time_str": time_ago,
                })
        return entries
    except Exception:
        return []


def format_relative_time(timestamp):
    """Format an epoch timestamp as a human-friendly relative time string."""
    diff = max(0, int(time.time() - timestamp))
    if diff < 10:
        return "Just now"
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
        return "Yesterday"
    if days < 7:
        return f"{days}d ago"
    return time.strftime("%b %d", time.localtime(timestamp))


def format_pango_entry(content):
    """
    Format entry content with glamorous Pango markup and appropriate icon.
    Escapes all XML entities to ensure valid Pango markup.
    """
    raw = content.strip()
    lines = raw.splitlines()
    first_line_clean = lines[0].strip() if lines else ""
    first_line_esc = html.escape(first_line_clean)
    line_count = len(lines)
    char_count = len(raw)

    # 1. URL Detection
    if re.match(r"^https?://\S+$", raw, re.IGNORECASE):
        icon = "🌐"
        # Extract domain
        m = re.match(r"^(https?://)([^/]+)(.*)$", raw, re.IGNORECASE)
        if m:
            proto, domain, path = m.groups()
            dom_esc = html.escape(domain)
            path_snippet = html.escape(path[:50] + ("…" if len(path) > 50 else ""))
    if is_url:
        icon = "🔗"
        if dom:
            dom_esc = html.escape(dom)
            path_snippet = html.escape(first_line_clean[len(dom):][:40])
            pango = f"<b>{dom_esc}</b>{path_snippet}"
        else:
            pango = f"{first_line_esc[:70]}"
        type_hint = "URL"
        return icon, pango, type_hint

    # 2. Hex Color Detection (#RGB, #RGBA, #RRGGBB, #RRGGBBAA)
    if re.match(r"^#(?:[0-9a-fA-F]{3,4}){1,2}$", raw):
        icon = "🎨"
        pango = f"<b>■ {raw}</b>"
        type_hint = "Color"
        return icon, pango, type_hint

    # 3. Email Detection
    if re.match(r"^[a-zA-Z0-9_.+-]+@[a-zA-Z0-9-]+\.[a-zA-Z0-9-.]+$", raw):
        icon = "✉️"
        pango = f"<b>{first_line_esc}</b>"
        type_hint = "Email"
        return icon, pango, type_hint

    # 4. JSON Detection
    if (raw.startswith("{") and raw.endswith("}")) or (raw.startswith("[") and raw.endswith("]")):
        try:
            parsed = json.loads(raw)
            icon = "📦"
            if isinstance(parsed, dict):
                count_str = f"{len(parsed)} keys"
            elif isinstance(parsed, list):
                count_str = f"{len(parsed)} items"
            else:
                count_str = "JSON"
            compact = json.dumps(parsed, separators=(", ", ":"))
            compact_esc = html.escape(compact[:65] + ("…" if len(compact) > 65 else ""))
            pango = f"{compact_esc}"
            type_hint = f"JSON • {count_str}"
            return icon, pango, type_hint
        except Exception:
            pass

    # 5. Code / CLI Command Detection
    code_keywords = (
        "def ", "fn ", "pub fn", "import ", "from ", "const ", "let ", "var ",
        "function", "class ", "struct ", "impl ", "interface ", "type ",
        "SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE TABLE",
        "curl ", "git ", "cargo ", "npm ", "pnpm ", "bun ", "yarn ", "docker ",
        "kubectl ", "brew ", "chmod ", "chown ", "ssh ", "grep ", "find ",
    )
    is_code = any(raw.startswith(kw) for kw in code_keywords) or (
        line_count > 1 and ("{" in raw or "}" in raw or "=>" in raw or ";" in raw)
    )
    if is_code:
        icon = "💻"
        first_esc = html.escape(first_line_clean[:55])
        if line_count > 1:
            second_clean = lines[1].strip()
            second_esc = html.escape(second_clean[:35] + ("…" if len(second_clean) > 35 else ""))
            pango = f"<b>{first_esc}</b> <i>↵ {second_esc}</i>"
        else:
            pango = f"<b>{first_esc}</b>"
        type_hint = f"{line_count} lines" if line_count > 1 else "code"
        return icon, pango, type_hint

    # 6. Multiline Text
    if line_count > 1:
        icon = "📄"
        first_esc = html.escape(first_line_clean[:60])
        second_clean = lines[1].strip()
        second_esc = html.escape(second_clean[:35] + ("…" if len(second_clean) > 35 else ""))
        pango = f"<b>{first_esc}</b> <i>↵ {second_esc}</i>"
        type_hint = f"{line_count} lines"
        return icon, pango, type_hint

    # 7. Single Line Text
    icon = "📝"
    snippet_esc = html.escape(first_line_clean[:75] + ("…" if len(first_line_clean) > 75 else ""))
    pango = f"{snippet_esc}"
    type_hint = f"{char_count} chars" if char_count > 40 else "text"
    return icon, pango, type_hint


def pbpaste_content():
    """Retrieve current system clipboard text via pbpaste."""
    try:
        res = subprocess.run(["pbpaste"], capture_output=True, text=True, timeout=1.0)
        return res.stdout
    except Exception:
        return ""


def pbcopy_content(text):
    """Write text directly to system clipboard via pbcopy."""
    try:
        proc = subprocess.Popen(["pbcopy"], stdin=subprocess.PIPE)
        proc.communicate(text.encode("utf-8"))
    except Exception:
        pass


def copy_item(clipy_bin, item_id, full_content=None):
    """Copy an entry back to the system clipboard via clipy copy and pbcopy."""
    copied = False
    if clipy_bin and item_id:
        try:
            res = subprocess.run([clipy_bin, "copy", item_id], capture_output=True, timeout=1.5)
            if res.returncode == 0:
                copied = True
        except Exception:
            pass

    if not copied and full_content is not None:
        pbcopy_content(full_content)


def delete_item(clipy_bin, item_id):
    """Delete an entry from clipy history."""
    if clipy_bin and item_id:
        try:
            subprocess.run([clipy_bin, "delete", item_id], capture_output=True, timeout=1.5)
        except Exception:
            pass


def emit_gui_frame(clipy_bin, entries, message_override=None):
    """Output the GUI protocol commands and rows to stdout."""
    print("\0prompt\x1fClipboard History")
    print("\0markup-rows\x1ftrue")
    print("\0multi-select\x1ftrue")

    msg = message_override or "↵ Copy to clipboard  •  ⇥ Multi-select  •  ^D Delete entry"
    print(f"\0message\x1f{msg}")

    if not entries:
        # Check current pbpaste
        cur = pbpaste_content().strip()
        if cur:
            icon, pango, hint = format_pango_entry(cur)
            meta = cur.replace("\n", " ")[:200]
            print(f"{pango}\0id\x1fcurrent\0icon\x1f{icon}\0info\x1fCurrent Clipboard\0meta\x1f{meta}")
        else:
            print(
                "<i>Clipboard history is empty. Copy text to see it here!</i>"
                "\0nonselectable\x1ftrue\0icon\x1f📋"
            )
        print("\0flush")
        sys.stdout.flush()
        return

    for entry in entries:
        item_id = entry["id"]
        content = entry["content"]
        time_str = entry.get("time_str")
        if not time_str and "updated_at" in entry:
            time_str = format_relative_time(entry["updated_at"])
        if not time_str:
            time_str = "Recent"

        icon, pango, type_hint = format_pango_entry(content)
        info_badge = f"{time_str} • {type_hint}"

        # Clean meta string for aerofi fuzzy matching
        meta_clean = content.replace("\n", " ").strip()[:500]
        # Avoid control characters in fields
        meta_clean = meta_clean.replace("\0", " ").replace("\x1f", " ")

        print(f"{pango}\0id\x1f{item_id}\0icon\x1f{icon}\0info\x1f{info_badge}\0meta\x1f{meta_clean}")

    print("\0flush")
    sys.stdout.flush()


def parse_event_line(line):
    """Parse a structured aerofi GUI event line."""
    line = line.lstrip("\x00").rstrip("\r\n")
    fields = line.split("\x1f")
    if not fields or fields[0] != "event":
        return {}

    event_type = fields[1] if len(fields) > 1 else ""
    parsed = {"type": event_type}
    for field in fields[2:]:
        if ":" in field:
            k, v = field.split(":", 1)
            parsed[k] = v
    return parsed


def find_entry_content(entries_map, sid):
    """Lookup content by full ID or prefix."""
    if not sid:
        return None
    if sid in entries_map:
        return entries_map[sid]
    for k, v in entries_map.items():
        if k.startswith(sid) or sid.startswith(k):
            return v
    return None


def main():
    clipy_bin = find_clipy_binary()
    db_path = get_history_db_path()

    if not clipy_bin:
        # Clipy not found - render setup instructions in the GUI
        print("\0prompt\x1fClipboard History")
        print("\0markup-rows\x1ftrue")
        print("\0message\x1fPress Enter on 'cargo install clipy' to copy the install command")
        print(
            "<b>clipy CLI is not installed</b>"
            "\0nonselectable\x1ftrue\0icon\x1f⚠️\0info\x1fMissing Dependency"
        )
        print(
            "<b>cargo install clipy</b>  "
            "<i>Copy command to install minimal clipboard daemon</i>"
            "\0id\x1finstall_cmd\0icon\x1f💡\0info\x1fPress Enter to copy"
        )
        cur = pbpaste_content().strip()
        if cur:
            icon, pango, _ = format_pango_entry(cur)
            print(f"{pango}\0id\x1fcurrent\0icon\x1f{icon}\0info\x1fCurrent System Clipboard")
        print("\0flush")
        sys.stdout.flush()

        # Handle selection in fallback mode
        raw = sys.stdin.buffer.readline()
        if raw:
            line = raw.decode("utf-8", errors="replace")
            event = parse_event_line(line)
            if event.get("id") == "install_cmd":
                pbcopy_content("cargo install clipy")
        return

    # Ensure daemon is running
    ensure_clipy_daemon(clipy_bin)

    # Initial load of entries
    def fetch_current_entries():
        entries = load_entries_from_db(db_path)
        if entries is None or not entries:
            entries = load_entries_from_cli(clipy_bin)
        return entries or []

    entries = fetch_current_entries()
    entries_map = {e["id"]: e["content"] for e in entries}

    # Emit the initial frame
    emit_gui_frame(clipy_bin, entries)

    # Interactive Event Loop
    while True:
        raw_line = sys.stdin.buffer.readline()
        if not raw_line:
            # Stdin EOF: aerofi closed or script terminated
            break

        line = raw_line.decode("utf-8", errors="replace")
        event = parse_event_line(line)
        if not event:
            continue

        event_type = event.get("type")

        # 1. Row Selection (Enter key)
        if event_type == "select":
            raw_ids = event.get("ids", "")
            selected_ids = [i.strip() for i in raw_ids.split(",") if i.strip()]
            if not selected_ids and event.get("id"):
                selected_ids = [event["id"].strip()]

            if selected_ids:
                if len(selected_ids) == 1:
                    sel_id = selected_ids[0]
                    content = find_entry_content(entries_map, sel_id)
                    copy_item(clipy_bin, sel_id, content)
                else:
                    # Multi-selection: join all selected items and copy to pbcopy
                    combined_pieces = []
                    for sid in selected_ids:
                        content = find_entry_content(entries_map, sid)
                        if content is not None:
                            combined_pieces.append(content.rstrip("\r\n"))
                        else:
                            # Try loading via clipy show
                            try:
                                res = subprocess.run(
                                    [clipy_bin, "show", sid],
                                    capture_output=True,
                                    text=True,
                                    timeout=1.0,
                                )
                                if res.returncode == 0:
                                    # Output format contains header up to '---'
                                    parts = res.stdout.split("---\n", 1)
                                    combined_pieces.append((parts[1] if len(parts) > 1 else res.stdout).rstrip())
                            except Exception:
                                pass
                    if combined_pieces:
                        full_text = "\n".join(combined_pieces)
                        pbcopy_content(full_text)
            break  # Exit after copying to close launcher

        # 2. Contextual Action (Ctrl+D / Delete)
        elif event_type == "action":
            act_id = event.get("id", "").strip()
            if act_id:
                delete_item(clipy_bin, act_id)
                # Re-fetch and re-emit list
                entries = fetch_current_entries()
                entries_map = {e["id"]: e["content"] for e in entries}
                emit_gui_frame(clipy_bin, entries, message_override="Item deleted from history")
            continue

        # 3. Custom text / other events
        elif event_type == "custom":
            custom_text = event.get("text", "")
            if custom_text:
                pbcopy_content(custom_text)
            break


if __name__ == "__main__":
    main()
