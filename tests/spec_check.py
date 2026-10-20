#!/usr/bin/env python3
"""Basic specification compliance checks; reads tracked files only.

1. Every feature folder in specs/ has spec.md, plan.md, and tasks.md.
2. AGENTS.md names an existing feature folder as the current scope.
3. Relative Markdown links in tracked files point to existing files and headings.
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REQUIRED = ("spec.md", "plan.md", "tasks.md")
# Upstream Spec Kit assets keep their own placeholder links.
SKIP_PREFIXES = (".specify/", ".agents/")
LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
FENCE = re.compile(r"^\s*(```|~~~)")
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")
_anchors = {}


def outside_fences(text):
    in_fence = False
    for number, line in enumerate(text.splitlines(), 1):
        if FENCE.match(line):
            in_fence = not in_fence
        elif not in_fence:
            yield number, line


def anchors(file):
    """Heading anchors as GitHub generates them, including -1, -2 for repeats."""
    if file not in _anchors:
        found, seen = set(), {}
        for _, line in outside_fences(file.read_text(encoding="utf-8")):
            match = HEADING.match(line)
            if not match:
                continue
            slug = re.sub(r"[^\w\- ]", "", match.group(1).lower()).replace(" ", "-")
            count = seen.get(slug, 0)
            seen[slug] = count + 1
            found.add(slug if count == 0 else f"{slug}-{count}")
        _anchors[file] = found
    return _anchors[file]


def tracked_markdown():
    out = subprocess.run(
        ["git", "ls-files", "*.md"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout
    return [p for p in out.splitlines() if not p.startswith(SKIP_PREFIXES)]


def broken_links(path):
    problems = []
    source = ROOT / path
    for number, line in outside_fences(source.read_text(encoding="utf-8")):
        for target in LINK.findall(line):
            if re.match(r"^[a-z][a-z0-9+.-]*:", target, re.I):
                continue
            file_part, _, fragment = target.partition("#")
            file = source.parent.joinpath(file_part).resolve() if file_part else source
            if not file.exists():
                problems.append(f"{path}:{number}: missing link target {target}")
            elif fragment and file.suffix == ".md" and fragment.lower() not in anchors(file):
                problems.append(f"{path}:{number}: missing heading #{fragment}")
    return problems


def main():
    problems = []
    features = sorted(p for p in (ROOT / "specs").iterdir() if p.is_dir())
    for feature in features:
        for name in REQUIRED:
            if not (feature / name).is_file():
                problems.append(f"specs/{feature.name}: missing {name}")

    scope = re.search(r"`(specs/[^`/]+)/?`", (ROOT / "AGENTS.md").read_text(encoding="utf-8"))
    if not scope or not (ROOT / scope.group(1)).is_dir():
        problems.append("AGENTS.md: current scope must name an existing specs/ folder")

    for path in tracked_markdown():
        problems.extend(broken_links(path))

    for problem in problems:
        print(f"FAIL: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"PASS: {len(features)} specifications, Markdown links, and heading anchors checked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
