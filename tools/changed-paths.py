#!/usr/bin/env python3
"""Decide whether a change can only have touched documentation.

The verify pipeline spends its wall-clock time compiling, and almost none of
that is needed for a README edit or a reworded issue template. This script
answers one question — *is every changed path a document?* — so the workflow
can run the contract tests and skip the rest.

The answer is one-directional on purpose. It can only report `true` when it has
proved that no changed path could affect code, fixtures, dependencies, the
dependency policy or the CI configuration itself; anything it does not
recognise, an unreadable diff, or an empty change set all report `false`, which
runs the whole pipeline. The opposite formulation — "run a job when one of its
inputs changed" — fails open: a new top-level directory matches no input list,
every job skips, and a change nobody looked at merges with a green run.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import PurePosixPath

# Paths that no job in `verify.yml` reads, and that no contract test asserts on.
# Everything else is treated as code. `.github/workflows/` and
# `.github/dependabot.yml` are deliberately absent: the first *is* the gate, and
# the second is read by `tools/tests/test_dependabot_contract.py`, so a change to
# either has to be verified by the pipeline it alters.
DOCUMENTATION_PREFIXES = (
    "docs/",
    ".github/ISSUE_TEMPLATE/",
)
DOCUMENTATION_NAMES = frozenset(
    {
        ".editorconfig",
        ".gitattributes",
        ".gitignore",
        "COPYING",
        "LICENSE",
    }
)


def is_documentation(path: str) -> bool:
    """Whether `path` can only be a document.

    Markdown is matched by suffix rather than by glob so that `fnmatch`'s
    `*` spanning path separators cannot quietly widen the answer.
    """
    candidate = PurePosixPath(path)
    if candidate.suffix == ".md":
        return True
    normalized = path.removeprefix("./")
    if any(normalized.startswith(prefix) for prefix in DOCUMENTATION_PREFIXES):
        return True
    return normalized in DOCUMENTATION_NAMES


def docs_only(changed: list[str]) -> bool:
    """Whether every changed path is documentation.

    An empty change set is not documentation: it means the diff could not be
    read, and an unreadable diff must not be the reason a gate stops running.
    """
    return bool(changed) and all(is_documentation(path) for path in changed)


def diff_range(event_name: str, base_ref: str | None) -> str:
    """The revision range to diff against for this event.

    A pull request is measured against its base branch, and a push against the
    commit it replaced.
    """
    if base_ref:
        return f"{base_ref}...HEAD"
    if event_name == "pull_request":
        base = os.environ.get("GITHUB_BASE_REF")
        if not base:
            raise RuntimeError("GITHUB_BASE_REF is missing for a pull_request run")
        return f"origin/{base}...HEAD"
    return "HEAD^..HEAD"


def changed_paths(event_name: str, base_ref: str | None) -> list[str]:
    """The paths this event changed, or `[]` when the diff cannot be read."""
    try:
        output = subprocess.check_output(
            ["git", "diff", "--name-only", diff_range(event_name, base_ref)],
            text=True,
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"could not diff against {diff_range(event_name, base_ref)}: {error}", file=sys.stderr)
        return []
    return [line for line in output.splitlines() if line.strip()]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--base",
        help="revision range base, overriding the one derived from the event",
    )
    parser.add_argument(
        "--changed",
        help="newline separated paths, instead of diffing; for tests",
    )
    arguments = parser.parse_args()

    if arguments.changed is not None:
        changed = [line for line in arguments.changed.splitlines() if line.strip()]
    else:
        changed = changed_paths(os.environ.get("GITHUB_EVENT_NAME", ""), arguments.base)

    verdict = "true" if docs_only(changed) else "false"
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as output:
            output.write(f"docs-only={verdict}\n")
            output.write(f"changed-count={len(changed)}\n")
    else:
        print(f"docs-only={verdict}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
