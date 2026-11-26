#!/usr/bin/env python3
"""Check repository-local Markdown and HTML links without network access."""

from __future__ import annotations

import html
import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
SKIP_DIRS = {".git", "target", "node_modules", ".nsh", "dist", "build"}
MARKDOWN_LINK = re.compile(
    r"!?(?:\[[^\]]*\])\(\s*(?:<([^>]+)>|((?:\\.|[^\s)])+))",
)
REFERENCE_LINK = re.compile(
    r"(?m)^\s{0,3}\[[^\]]+\]:\s*(?:<([^>]+)>|(\S+))",
)
INLINE_CODE = re.compile(r"(?<!`)`+[^`\n]+`+(?!`)")
HTML_ATTR = re.compile(
    r"""\b(?:href|src|srcset)\s*=\s*["']([^"']+)["']""",
    re.IGNORECASE,
)
FENCED_BLOCK = re.compile(
    r"(?ms)^([ \t]{0,3})(```+|~~~+)[^\n]*\n.*?^\1\2[ \t]*$",
)


def markdown_files() -> list[Path]:
    return sorted(
        path
        for path in ROOT.rglob("*.md")
        if not any(part in SKIP_DIRS for part in path.relative_to(ROOT).parts)
    )


def strip_fenced_blocks(source: str) -> str:
    return FENCED_BLOCK.sub(
        lambda match: "\n" * match.group(0).count("\n"),
        source,
    )


def strip_inline_code(source: str) -> str:
    return INLINE_CODE.sub(
        lambda match: "".join(
            "\n" if char == "\n" else " " for char in match.group(0)
        ),
        source,
    )


def destinations(source: str) -> list[tuple[int, str]]:
    source = strip_inline_code(strip_fenced_blocks(source))
    found: list[tuple[int, str]] = []
    for pattern in (MARKDOWN_LINK, REFERENCE_LINK, HTML_ATTR):
        for match in pattern.finditer(source):
            value = next((part for part in match.groups() if part), "")
            line = source.count("\n", 0, match.start()) + 1
            if pattern is HTML_ATTR and match.group(0).lower().startswith("srcset"):
                for candidate in value.split(","):
                    parts = candidate.strip().split()
                    if parts:
                        found.append((line, html.unescape(parts[0])))
                continue
            found.append((line, html.unescape(value.strip())))
    return found


def is_external(destination: str) -> bool:
    if not destination or destination.startswith("#"):
        return True
    parsed = urlsplit(destination)
    return bool(parsed.scheme or parsed.netloc)


def resolve_target(source: Path, destination: str) -> Path | None:
    if is_external(destination):
        return None
    parsed = urlsplit(destination)
    target = unquote(parsed.path)
    if not target:
        return None
    if target.startswith("/"):
        return (ROOT / target.lstrip("/")).resolve()
    return (source.parent / target).resolve()


def main() -> int:
    failures: list[str] = []
    files = markdown_files()
    for source in files:
        text = source.read_text(encoding="utf-8")
        for line, destination in destinations(text):
            target = resolve_target(source, destination)
            if target is None:
                continue
            try:
                target.relative_to(ROOT)
            except ValueError:
                failures.append(
                    f"{source.relative_to(ROOT)}:{line}: link escapes repository: {destination}"
                )
                continue
            if not target.exists():
                failures.append(
                    f"{source.relative_to(ROOT)}:{line}: missing local target: {destination}"
                )

    if failures:
        print("\n".join(failures), file=sys.stderr)
        print(f"Found {len(failures)} broken local documentation link(s).", file=sys.stderr)
        return 1

    print(f"Checked local Markdown/HTML links in {len(files)} Markdown files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
