"""Targeted key rewrite helpers that preserve comments and unknown lines."""

from __future__ import annotations

import re


def set_ini_key(text: str, section: str | None, key: str, value: str) -> str:
    """Set key=value in an INI-like file. section=None for flat / pre-section keys."""
    lines = text.splitlines(keepends=True)
    section_re = re.compile(r"^\[([^\]]+)\]\s*$")
    key_re = re.compile(rf"^(\s*)({re.escape(key)})(\s*=\s*)(.*)$")

    out: list[str] = []
    cur: str | None = None
    replaced = False
    i = 0
    while i < len(lines):
        line = lines[i]
        bare = line.rstrip("\r\n")
        sm = section_re.match(bare.strip()) if bare.strip().startswith("[") else None
        if sm:
            # Leaving a target section without replacement → insert key first
            if (
                section is not None
                and cur == section
                and not replaced
            ):
                out.append(f"{key}={value}\n")
                replaced = True
            cur = sm.group(1)
            out.append(line)
            i += 1
            continue

        in_target = (section is None and cur is None) or (section is not None and cur == section)
        # Also allow section=None to match keys in any section-less region only
        if section is None:
            in_target = cur is None

        if in_target and not replaced:
            km = key_re.match(bare)
            if km:
                nl = "\r\n" if line.endswith("\r\n") else ("\n" if line.endswith("\n") else "")
                out.append(f"{km.group(1)}{km.group(2)}{km.group(3)}{value}{nl}")
                replaced = True
                i += 1
                continue

        out.append(line)
        i += 1

    if not replaced:
        if section is not None:
            # Still inside section at EOF?
            if cur == section:
                if out and not str(out[-1]).endswith("\n"):
                    out.append("\n")
                out.append(f"{key}={value}\n")
            else:
                if out and not str(out[-1]).endswith("\n"):
                    out.append("\n")
                out.append(f"\n[{section}]\n{key}={value}\n")
        else:
            if out and not str(out[-1]).endswith("\n"):
                out.append("\n")
            out.append(f"{key}={value}\n")
    return "".join(out)


def get_ini_key(text: str, section: str | None, key: str) -> str | None:
    cur: str | None = None
    key_re = re.compile(rf"^\s*{re.escape(key)}\s*=\s*(.*?)\s*$")
    section_re = re.compile(r"^\[([^\]]+)\]\s*$")
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("[") and s.endswith("]"):
            sm = section_re.match(s)
            cur = sm.group(1) if sm else cur
            continue
        if section is None:
            if cur is not None:
                continue
        elif cur != section:
            continue
        m = key_re.match(line)
        if m:
            return m.group(1)
    return None


def set_flat_key(text: str, key: str, value: str, *, sep: str = "=") -> str:
    lines = text.splitlines(keepends=True)
    key_re = re.compile(rf"^(\s*)({re.escape(key)})(\s*{re.escape(sep)}\s*)(.*)$")
    out: list[str] = []
    replaced = False
    for line in lines:
        bare = line.rstrip("\r\n")
        m = key_re.match(bare)
        if m and not replaced:
            nl = "\r\n" if line.endswith("\r\n") else ("\n" if line.endswith("\n") else "")
            out.append(f"{m.group(1)}{m.group(2)}{m.group(3)}{value}{nl}")
            replaced = True
        else:
            out.append(line)
    if not replaced:
        if out and not str(out[-1]).endswith("\n"):
            out.append("\n")
        out.append(f"{key}{sep}{value}\n")
    return "".join(out)


def get_flat_key(text: str, key: str, *, sep: str = "=") -> str | None:
    key_re = re.compile(rf"^\s*{re.escape(key)}\s*{re.escape(sep)}\s*(.*?)\s*$")
    for line in text.splitlines():
        if line.lstrip().startswith("#"):
            continue
        m = key_re.match(line)
        if m:
            return m.group(1)
    return None


def set_hypr_key(text: str, block_path: list[str], key: str, value: str) -> str:
    if not block_path:
        return set_flat_key(text, key, value, sep="=")

    lines = text.splitlines(keepends=True)
    open_re = re.compile(r"^(\s*)([A-Za-z0-9_-]+)(\s*)\{\s*(#.*)?$")
    close_re = re.compile(r"^(\s*)\}\s*(#.*)?$")
    key_re = re.compile(rf"^(\s*)({re.escape(key)})(\s*=\s*)(.*)$")

    name_stack: list[str] = []
    block_start = None
    block_end = None
    target_depth = None

    for i, line in enumerate(lines):
        bare = line.rstrip("\r\n")
        om = open_re.match(bare)
        if om:
            name_stack.append(om.group(2))
            if name_stack == block_path and block_start is None:
                block_start = i
                target_depth = len(name_stack)
            continue
        if close_re.match(bare):
            if (
                target_depth is not None
                and len(name_stack) == target_depth
                and block_end is None
            ):
                block_end = i
            if name_stack:
                name_stack.pop()

    if block_start is None or block_end is None:
        return text

    out: list[str] = []
    replaced = False
    for i, line in enumerate(lines):
        if block_start < i < block_end and not replaced:
            bare = line.rstrip("\r\n")
            nest = 0
            for j in range(block_start + 1, i):
                b = lines[j].rstrip("\r\n")
                if open_re.match(b):
                    nest += 1
                elif close_re.match(b):
                    nest -= 1
            if nest == 0:
                m = key_re.match(bare)
                if m:
                    nl = "\r\n" if line.endswith("\r\n") else ("\n" if line.endswith("\n") else "")
                    rest = m.group(4)
                    comment = ""
                    if "#" in rest:
                        # keep trailing comment
                        before, _, after = rest.partition("#")
                        comment = "  #" + after
                    out.append(f"{m.group(1)}{key} = {value}{comment}{nl}")
                    replaced = True
                    continue
        if i == block_end and not replaced:
            close_bare = line.rstrip("\r\n")
            cm = close_re.match(close_bare)
            indent = (cm.group(1) if cm else "") + "    "
            out.append(f"{indent}{key} = {value}\n")
            replaced = True
        out.append(line)
    return "".join(out)


def get_hypr_key(text: str, block_path: list[str], key: str) -> str | None:
    lines = text.splitlines()
    open_re = re.compile(r"^(\s*)([A-Za-z0-9_-]+)(\s*)\{\s*(#.*)?$")
    close_re = re.compile(r"^(\s*)\}\s*(#.*)?$")
    key_re = re.compile(rf"^\s*{re.escape(key)}\s*=\s*(.*?)\s*$")
    name_stack: list[str] = []
    for i, line in enumerate(lines):
        bare = line.rstrip("\r\n")
        om = open_re.match(bare)
        if om:
            name_stack.append(om.group(2))
            continue
        if close_re.match(bare):
            if name_stack:
                name_stack.pop()
            continue
        if name_stack != block_path:
            continue
        # ensure not inside nested block within path — stack exact match is enough
        m = key_re.match(bare)
        if m:
            val = m.group(1).strip()
            if "#" in val:
                val = val.split("#", 1)[0].rstrip()
            return val
    return None


def set_xsettings(text: str, key: str, value: str) -> str:
    lines = text.splitlines(keepends=True)
    key_re = re.compile(rf"^(\s*)({re.escape(key)})(\s+)(\".*\"|\S+)(\s*)$")
    out: list[str] = []
    replaced = False
    quoted = f'"{value}"'
    for line in lines:
        bare = line.rstrip("\r\n")
        m = key_re.match(bare)
        if m and not replaced:
            nl = "\r\n" if line.endswith("\r\n") else ("\n" if line.endswith("\n") else "")
            out.append(f"{m.group(1)}{m.group(2)}{m.group(3)}{quoted}{nl}")
            replaced = True
        else:
            out.append(line)
    if not replaced:
        if out and not str(out[-1]).endswith("\n"):
            out.append("\n")
        out.append(f'{key} "{value}"\n')
    return "".join(out)


def get_xsettings(text: str, key: str) -> str | None:
    key_re = re.compile(rf'^\s*{re.escape(key)}\s+"([^"]*)"')
    key_re2 = re.compile(rf"^\s*{re.escape(key)}\s+(\S+)")
    for line in text.splitlines():
        m = key_re.match(line)
        if m:
            return m.group(1)
        m = key_re2.match(line)
        if m:
            return m.group(1).strip('"')
    return None


def replace_span(text: str, start: int, end: int, new: str) -> str:
    return text[:start] + new + text[end:]
