#!/usr/bin/env python3
"""Download immutable plugin release assets through the GitHub API."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess


MAX_ASSET_BYTES = 17 * 1024 * 1024


def load_release(repository: str, tag: str, draft: bool, run=subprocess.run) -> dict:
    response = run(
        [
            "gh",
            "release",
            "view",
            tag,
            "--repo",
            repository,
            "--json",
            "databaseId,tagName,isDraft",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    release = json.loads(response.stdout)
    if release.get("tagName") != tag:
        raise ValueError(f"release lookup returned the wrong tag for {tag}")
    if release.get("isDraft") is not draft:
        kind = "draft" if draft else "published"
        raise ValueError(f"release {tag} is not a {kind} release")
    release_id = release.get("databaseId")
    if not isinstance(release_id, int):
        raise ValueError(f"release {tag} has no database ID")
    release["assets"] = json.loads(
        run(
            [
                "gh",
                "api",
                f"repos/{repository}/releases/{release_id}",
                "--jq",
                ".assets",
            ],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    return release


def fetch(
    repository: str,
    tag: str,
    output: Path,
    names: set[str] | None = None,
    draft: bool = False,
    run=subprocess.run,
) -> None:
    output.mkdir(parents=True, exist_ok=True)
    release = load_release(repository, tag, draft, run)
    assets = release["assets"]
    if not isinstance(assets, list) or not assets:
        raise ValueError(f"release {tag} has no downloadable assets")
    selected = [
        asset
        for asset in assets
        if names is None
        or asset["name"] in names
        or asset["name"] == "release-state.json"
    ]
    if names is not None:
        missing = names - {asset["name"] for asset in selected}
        if missing:
            raise ValueError(f"release {tag} is missing assets: {sorted(missing)}")
    for asset in selected:
        if not isinstance(asset.get("id"), int) or asset.get("size", 0) <= 0:
            raise ValueError(f"release {tag} contains an invalid asset")
        if asset["size"] > MAX_ASSET_BYTES:
            raise ValueError(f"{asset['name']} exceeds the plugin asset limit")
        payload = run(
            [
                "gh",
                "api",
                f"repos/{repository}/releases/assets/{asset['id']}",
                "--header",
                "Accept: application/octet-stream",
            ],
            check=True,
            capture_output=True,
        ).stdout
        if len(payload) != asset["size"]:
            raise ValueError(f"{asset['name']} download is incomplete")
        (output / asset["name"]).write_bytes(payload)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--names", help="comma-separated asset names")
    parser.add_argument(
        "--draft",
        action="store_true",
        help="read assets from a draft release instead of a published tag",
    )
    args = parser.parse_args()
    names = set(args.names.split(",")) if args.names else None
    fetch(args.repository, args.tag, args.output, names, args.draft)


if __name__ == "__main__":
    main()
