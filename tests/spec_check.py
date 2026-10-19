#!/usr/bin/env python3
"""Basic specification compliance checks; reads tracked files only.

1. Every feature folder in specs/ has spec.md, plan.md, and tasks.md.
2. AGENTS.md names an existing feature folder as the current scope.
3. Relative Markdown links in tracked files point to existing files.
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


def tracked_markdown():
    out = subprocess.run(
        ["git", "ls-files", "*.md"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout
    return [p for p in out.splitlines() if not p.startswith(SKIP_PREFIXES)]


def broken_links(path):
    problems = []
    in_fence = False
    for number, line in enumerate((ROOT / path).read_text(encoding="utf-8").splitlines(), 1):
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        for target in LINK.findall(line):
            if re.match(r"^[a-z][a-z0-9+.-]*:", target, re.I) or target.startswith("#"):
                continue
            file_part = target.split("#", 1)[0]
            if not (ROOT / path).parent.joinpath(file_part).resolve().exists():
                problems.append(f"{path}:{number}: missing link target {target}")
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
    print(f"PASS: {len(features)} specifications and Markdown links checked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
