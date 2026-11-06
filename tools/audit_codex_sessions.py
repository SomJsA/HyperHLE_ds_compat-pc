#!/usr/bin/env python3
"""Audit complete Codex JSONL rollouts and emit a readable dialogue index.

The raw rollout remains the source of truth. This tool streams it line by line,
validates every JSON record, records a SHA-256 digest, and extracts user and
assistant text plus tool calls/results without truncating message bodies.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable


def content_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    parts: list[str] = []
    for item in content:
        if isinstance(item, str):
            parts.append(item)
        elif isinstance(item, dict):
            text = item.get("text")
            if isinstance(text, str):
                parts.append(text)
    return "\n".join(parts)


def walk(value: Any) -> Iterable[dict[str, Any]]:
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


def audit_session(path: Path, output_dir: Path) -> dict[str, Any]:
    digest = hashlib.sha256()
    type_counts: Counter[str] = Counter()
    role_counts: Counter[str] = Counter()
    dialogue: list[str] = []
    user_requests: list[str] = []
    assistant_messages: list[str] = []
    line_count = 0
    message_count = 0
    tool_count = 0
    invalid_lines: list[int] = []

    with path.open("rb") as raw:
        for line_count, raw_line in enumerate(raw, 1):
            digest.update(raw_line)
            if not raw_line.strip():
                continue
            try:
                record = json.loads(raw_line)
            except json.JSONDecodeError:
                invalid_lines.append(line_count)
                continue

            payload = record.get("payload", {})
            record_type = str(record.get("type", ""))
            payload_type = str(payload.get("type", ""))
            type_counts[f"{record_type}:{payload_type}"] += 1

            if payload_type == "message":
                role = str(payload.get("role", "unknown"))
                role_counts[role] += 1
                if role in {"user", "assistant"}:
                    text = content_text(payload.get("content"))
                    if text:
                        message_count += 1
                        phase = payload.get("phase")
                        suffix = f" phase={phase}" if phase else ""
                        item = f"\n## {role}{suffix} | line {line_count}\n\n{text.rstrip()}\n"
                        dialogue.append(item)
                        if role == "user":
                            user_requests.append(item)
                        else:
                            assistant_messages.append(item)
                continue

            if payload_type in {"function_call", "custom_tool_call"}:
                tool_count += 1
                name = payload.get("name", "tool")
                arguments = payload.get("arguments") or payload.get("input") or ""
                if not isinstance(arguments, str):
                    arguments = json.dumps(arguments, ensure_ascii=False, indent=2)
                dialogue.append(
                    f"\n### tool call {name} | line {line_count}\n\n"
                    f"```text\n{arguments.rstrip()}\n```\n"
                )
                continue

            if payload_type in {"function_call_output", "custom_tool_call_output"}:
                tool_count += 1
                output = payload.get("output", "")
                if not isinstance(output, str):
                    output = json.dumps(output, ensure_ascii=False, indent=2)
                dialogue.append(
                    f"\n### tool result | line {line_count}\n\n"
                    f"```text\n{output.rstrip()}\n```\n"
                )
                continue

            # Some Codex versions wrap command executions in response items.
            for candidate in walk(payload):
                candidate_type = candidate.get("type")
                if candidate_type in {"commandExecution", "mcpToolCall"}:
                    tool_count += 1
                    command = candidate.get("command")
                    if command:
                        dialogue.append(
                            f"\n### command | line {line_count}\n\n"
                            f"```text\n{command.rstrip()}\n```\n"
                        )
                    output = candidate.get("output")
                    if isinstance(output, dict):
                        output = output.get("text")
                    if isinstance(output, str) and output:
                        dialogue.append(
                            f"\n### command result | line {line_count}\n\n"
                            f"```text\n{output.rstrip()}\n```\n"
                        )
                    break

    stem = path.name.removesuffix(".jsonl")
    transcript_path = output_dir / f"{stem}.dialogue.md"
    transcript_path.write_text(
        f"# Codex rollout dialogue index\n\n"
        f"- Source: `{path}`\n"
        f"- Bytes: `{path.stat().st_size}`\n"
        f"- SHA-256: `{digest.hexdigest()}`\n"
        f"- JSONL lines: `{line_count}`\n"
        f"- User/assistant messages: `{message_count}`\n"
        f"- Tool records: `{tool_count}`\n"
        f"- Invalid JSON lines: `{len(invalid_lines)}`\n"
        + "".join(dialogue),
        encoding="utf-8",
    )
    user_path = output_dir / f"{stem}.user-requests.md"
    user_path.write_text(
        f"# User requests from `{path.name}`\n\n"
        f"- Source SHA-256: `{digest.hexdigest()}`\n"
        f"- JSONL lines scanned: `{line_count}`\n"
        + "".join(user_requests),
        encoding="utf-8",
    )
    assistant_path = output_dir / f"{stem}.assistant-messages.md"
    assistant_path.write_text(
        f"# Assistant messages from `{path.name}`\n\n"
        f"- Source SHA-256: `{digest.hexdigest()}`\n"
        f"- JSONL lines scanned: `{line_count}`\n"
        + "".join(assistant_messages),
        encoding="utf-8",
    )

    return {
        "source": str(path),
        "bytes": path.stat().st_size,
        "sha256": digest.hexdigest(),
        "jsonl_lines": line_count,
        "user_assistant_messages": message_count,
        "tool_records": tool_count,
        "invalid_json_lines": invalid_lines,
        "record_type_counts": dict(sorted(type_counts.items())),
        "role_counts": dict(sorted(role_counts.items())),
        "dialogue_index": str(transcript_path),
        "dialogue_index_bytes": transcript_path.stat().st_size,
        "user_requests": str(user_path),
        "user_requests_bytes": user_path.stat().st_size,
        "assistant_messages": str(assistant_path),
        "assistant_messages_bytes": assistant_path.stat().st_size,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("sessions", nargs="+", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    args.output_dir.mkdir(parents=True, exist_ok=True)
    results = [audit_session(path, args.output_dir) for path in args.sessions]
    manifest = args.output_dir / "session-audit.json"
    manifest.write_text(
        json.dumps({"sessions": results}, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({"manifest": str(manifest), "sessions": results}, indent=2))


if __name__ == "__main__":
    main()
