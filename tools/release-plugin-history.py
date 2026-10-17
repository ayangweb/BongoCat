#!/usr/bin/env python3
"""Fetch signed catalogs from GitHub without mutating releases or needing a PAT.

Packaging owns catalog validation and version policy. This adapter only fetches
published history; API errors must fail closed, not look like a first release.
"""

import argparse
import json
from pathlib import Path
import re
import subprocess


def fetch_history(repository, tag, output, run=subprocess.run, allow_existing_tag=False):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid release repository")
    response = run(
        ["gh", "api", f"repos/{repository}/releases", "--paginate", "--slurp"],
        check=True,
        capture_output=True,
    )
    releases = [release for page in json.loads(response.stdout) for release in page]
    if not allow_existing_tag and any(release["tag_name"] == tag for release in releases):
        raise ValueError("release or draft already exists; inspect it before retrying")
    # A fresh directory prevents leftovers or a partial prior download being
    # mistaken for the complete immutable history.
    output.mkdir(parents=True, exist_ok=False)
    for release in releases:
        if release["draft"] or release["prerelease"]:
            continue
        for asset in release["assets"]:
            if asset["name"] != "plugins.json":
                continue
            if asset["size"] <= 0 or asset["size"] > 1024 * 1024:
                raise ValueError("published plugin catalog exceeds the runtime limit")
            asset_id = int(asset["id"])
            response = run(
                [
                    "gh", "api", f"repos/{repository}/releases/assets/{asset_id}",
                    "--header", "Accept: application/octet-stream",
                ],
                check=True,
                capture_output=True,
            )
            if len(response.stdout) != asset["size"]:
                raise ValueError("published plugin catalog download is incomplete")
            (output / f"{asset_id}.json").write_bytes(response.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--allow-existing-tag",
        action="store_true",
        help="the catalog is being updated on the existing product release",
    )
    args = parser.parse_args()
    fetch_history(
        args.repository, args.tag, args.output, allow_existing_tag=args.allow_existing_tag
    )


if __name__ == "__main__":
    main()
