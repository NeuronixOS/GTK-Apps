#!/usr/bin/env python3
"""Cursor SDK stdin/stdout JSON bridge for gtk-neurond."""

from __future__ import annotations

import json
import os
import sys


def main() -> int:
    line = sys.stdin.readline()
    if not line.strip():
        print(json.dumps({"error": "empty request"}))
        return 1
    try:
        req = json.loads(line)
    except json.JSONDecodeError as e:
        print(json.dumps({"error": f"invalid json: {e}"}))
        return 1

    api_key = req.get("api_key") or os.environ.get("CURSOR_API_KEY")
    if not api_key:
        print(json.dumps({"error": "missing Cursor API key"}))
        return 1

    system = req.get("system") or ""
    user = req.get("user") or ""
    history = req.get("history") or []
    cwd = req.get("cwd") or os.getcwd()

    prompt_parts = []
    if system:
        prompt_parts.append(system)
    for item in history:
        role = item.get("role", "user")
        text = item.get("text", "")
        prompt_parts.append(f"{role}: {text}")
    prompt_parts.append(user)
    prompt = "\n\n".join(prompt_parts)

    try:
        from cursor_sdk import Agent, AgentOptions, LocalAgentOptions
    except ImportError:
        print(
            json.dumps(
                {
                    "error": "cursor-sdk not installed; pip install cursor-sdk",
                }
            )
        )
        return 1

    try:
        result = Agent.prompt(
            prompt,
            AgentOptions(
                api_key=api_key,
                model="composer-2.5",
                local=LocalAgentOptions(cwd=cwd),
            ),
        )
        text = getattr(result, "result", None) or str(result)
        print(json.dumps({"text": text, "status": getattr(result, "status", None)}))
        return 0
    except Exception as e:  # noqa: BLE001 — bridge must always return JSON
        print(json.dumps({"error": str(e)}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
