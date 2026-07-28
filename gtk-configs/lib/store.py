"""In-memory config document store with dirty tracking."""

from __future__ import annotations

from pathlib import Path

from . import colors_fmt, io_keys, root as rootmod


class ConfigStore:
    def __init__(self, root: Path):
        self.root = root.resolve()
        self._texts: dict[str, str] = {}
        self._dirty: set[str] = set()
        self._orig: dict[str, str] = {}

    def rel(self, path: Path | str) -> str:
        p = Path(path)
        if p.is_absolute():
            return str(p.resolve().relative_to(self.root))
        return str(p)

    def abs(self, rel: str) -> Path:
        return self.root / rel

    def load_all(self) -> None:
        self._texts.clear()
        self._dirty.clear()
        self._orig.clear()
        for path in rootmod.list_text_files(self.root):
            rel = self.rel(path)
            try:
                text = path.read_text(encoding="utf-8")
            except (UnicodeDecodeError, OSError):
                continue
            self._texts[rel] = text
            self._orig[rel] = text

    def ensure(self, rel: str) -> str:
        if rel not in self._texts:
            path = self.abs(rel)
            if path.is_file():
                try:
                    text = path.read_text(encoding="utf-8")
                except (UnicodeDecodeError, OSError):
                    text = ""
            else:
                text = ""
            self._texts[rel] = text
            self._orig[rel] = text
        return self._texts[rel]

    def get_text(self, rel: str) -> str:
        return self.ensure(rel)

    def set_text(self, rel: str, text: str) -> None:
        self.ensure(rel)
        if self._texts[rel] != text:
            self._texts[rel] = text
            if text != self._orig.get(rel, ""):
                self._dirty.add(rel)
            else:
                self._dirty.discard(rel)

    def is_dirty(self, rel: str | None = None) -> bool:
        if rel is None:
            return bool(self._dirty)
        return rel in self._dirty

    def dirty_files(self) -> list[str]:
        return sorted(self._dirty)

    def save(self, rels: list[str] | None = None) -> list[str]:
        """Write dirty files; return list saved."""
        targets = list(rels) if rels is not None else list(self._dirty)
        saved: list[str] = []
        for rel in targets:
            if rel not in self._dirty and rels is None:
                continue
            path = self.abs(rel)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(self._texts[rel], encoding="utf-8")
            self._orig[rel] = self._texts[rel]
            self._dirty.discard(rel)
            saved.append(rel)
        return saved

    def discard(self, rels: list[str] | None = None) -> None:
        targets = list(rels) if rels is not None else list(self._dirty)
        for rel in targets:
            if rel in self._orig:
                self._texts[rel] = self._orig[rel]
            self._dirty.discard(rel)

    def reload_from_disk(self) -> None:
        self.load_all()

    # --- typed helpers ---

    def get_ini(self, rel: str, section: str | None, key: str) -> str | None:
        return io_keys.get_ini_key(self.get_text(rel), section, key)

    def set_ini(self, rel: str, section: str | None, key: str, value: str) -> None:
        self.set_text(rel, io_keys.set_ini_key(self.get_text(rel), section, key, value))

    def get_flat(self, rel: str, key: str) -> str | None:
        return io_keys.get_flat_key(self.get_text(rel), key)

    def set_flat(self, rel: str, key: str, value: str) -> None:
        self.set_text(rel, io_keys.set_flat_key(self.get_text(rel), key, value))

    def get_hypr(self, rel: str, block: list[str], key: str) -> str | None:
        return io_keys.get_hypr_key(self.get_text(rel), block, key)

    def set_hypr(self, rel: str, block: list[str], key: str, value: str) -> None:
        self.set_text(rel, io_keys.set_hypr_key(self.get_text(rel), block, key, value))

    def get_xsettings(self, rel: str, key: str) -> str | None:
        return io_keys.get_xsettings(self.get_text(rel), key)

    def set_xsettings(self, rel: str, key: str, value: str) -> None:
        self.set_text(rel, io_keys.set_xsettings(self.get_text(rel), key, value))

    def get_toml_simple(self, rel: str, key: str) -> str | None:
        """Simple top-level key = \"value\" or key = value."""
        return io_keys.get_flat_key(self.get_text(rel), key)

    def set_toml_simple(self, rel: str, key: str, value: str) -> None:
        text = self.get_text(rel)
        # quote strings if not already typed as number/bool
        v = value
        if not re_is_bare_toml(v):
            if not (v.startswith('"') and v.endswith('"')):
                v = f'"{value}"'
        self.set_text(rel, io_keys.set_flat_key(text, key, v))

    def scan_colors(self) -> list[colors_fmt.ColorHit]:
        hits: list[colors_fmt.ColorHit] = []
        color_files = [
            "hypr/hyprland.conf",
            "waybar/style.css",
            "waybar/config",
            "fuzzel/fuzzel.ini",
            "mako/config",
            "gtk-apps/custom-profiles.json",
        ]
        for rel in color_files:
            if not self.abs(rel).is_file() and rel not in self._texts:
                continue
            text = self.get_text(rel)
            fuzzel = rel.endswith("fuzzel.ini")
            hits.extend(colors_fmt.scan_text(rel, text, fuzzel_keys=fuzzel))
        return hits

    def set_color_hit(self, hit: colors_fmt.ColorHit, rgba: colors_fmt.RGBA) -> None:
        text = self.get_text(hit.path_key)
        # Re-scan to find current span (offsets may have shifted); match by context token near start
        hits = colors_fmt.scan_text(
            hit.path_key, text, fuzzel_keys=hit.path_key.endswith("fuzzel.ini")
        )
        # Prefer exact start/end if still valid
        target = None
        for h in hits:
            if h.start == hit.start and h.end == hit.end and h.token == hit.token:
                target = h
                break
        if target is None:
            for h in hits:
                if h.token == hit.token and h.context == hit.context:
                    target = h
                    break
        if target is None:
            for h in hits:
                if h.kind == hit.kind and h.token == hit.token:
                    target = h
                    break
        if target is None:
            return
        new_tok = colors_fmt.format_color(
            rgba, target.kind, had_alpha=(target.kind in ("fuzzel",) or len(target.token) > 7)
        )
        # better: use rewrite_token
        new_tok = colors_fmt.rewrite_token(target.token, rgba)
        self.set_text(
            hit.path_key, io_keys.replace_span(text, target.start, target.end, new_tok)
        )


def re_is_bare_toml(v: str) -> bool:
    v = v.strip()
    if v in ("true", "false"):
        return True
    try:
        float(v)
        return True
    except ValueError:
        return False
