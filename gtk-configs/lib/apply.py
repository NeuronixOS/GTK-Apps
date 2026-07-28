"""Best-effort session reload after saving configs."""

from __future__ import annotations

import os
import shutil
import subprocess


def _run(cmd: list[str]) -> tuple[bool, str]:
    try:
        r = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=8,
            env=os.environ.copy(),
        )
        if r.returncode == 0:
            return True, (r.stdout or "").strip()
        return False, (r.stderr or r.stdout or f"exit {r.returncode}").strip()
    except (OSError, subprocess.TimeoutExpired) as e:
        return False, str(e)


def reload_hypr() -> tuple[bool, str]:
    if not shutil.which("hyprctl"):
        return False, "hyprctl not found"
    return _run(["hyprctl", "reload"])


def reload_waybar() -> tuple[bool, str]:
    # SIGUSR2 = reload styles/config for waybar
    if shutil.which("killall"):
        ok, msg = _run(["killall", "-SIGUSR2", "waybar"])
        if ok:
            return True, "waybar SIGUSR2"
        # killall returns non-zero if no process
        return False, msg or "waybar not running"
    return False, "killall not found"


def reload_mako() -> tuple[bool, str]:
    if shutil.which("makoctl"):
        return _run(["makoctl", "reload"])
    if shutil.which("killall"):
        return _run(["killall", "-SIGUSR2", "mako"])
    return False, "makoctl/killall not found"


def apply_for_files(rels: list[str]) -> list[str]:
    """Reload components affected by saved relative paths. Return status lines."""
    notes: list[str] = []
    joined = " ".join(rels)
    if any(r.startswith("hypr/") for r in rels):
        ok, msg = reload_hypr()
        notes.append(f"hypr: {'ok' if ok else 'skip'} ({msg})")
    if any(r.startswith("waybar/") for r in rels):
        ok, msg = reload_waybar()
        notes.append(f"waybar: {'ok' if ok else 'skip'} ({msg})")
    if any(r.startswith("mako/") for r in rels):
        ok, msg = reload_mako()
        notes.append(f"mako: {'ok' if ok else 'skip'} ({msg})")
    if "fuzzel/fuzzel.ini" in rels:
        notes.append("fuzzel: restart launcher to pick up changes")
    if not notes and joined:
        notes.append("saved (no session reload needed)")
    return notes
