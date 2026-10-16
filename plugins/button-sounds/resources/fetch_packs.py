#!/usr/bin/env python3
"""Fetch the button-sounds asset set and regenerate `src/packs.rs`.

The plugin plays from sound sets copied out of Thock's public soundpack registry
(https://github.com/kamillobinski/thock-soundpacks) and committed here, so a user
never has to be online to hear anything. This script is the recipe that produced
those bytes: it downloads every soundpack the registry publishes, keeps only the
sounds this plugin can actually address, writes them under `resources/packs/`,
and rewrites the two files that describe them — `resources/index.json` for
provenance and `src/packs.rs` for the build.

Only the part of a pack this plugin can play is kept. A pack's per-key sounds
(space, enter, one file per letter) are deliberately dropped: the host hands a
plugin anonymous device activity and no key identity (ADR-0091), so the keyboard
plays from the pack's `default` group and the mouse from its left/right groups.
Keeping the rest would multiply the size of every set for sounds nothing could
ever address.

A pack whose license does not permit redistribution is refused rather than
reviewed case by case: Thock's own `Mamba Elite` pack is licensed for the Thock
application alone, so it is skipped and reported.

Run from anywhere:

    python3 plugins/button-sounds/resources/fetch_packs.py
"""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import tempfile
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path

REGISTRY_MANIFEST = (
    "https://raw.githubusercontent.com/kamillobinski/thock-soundpacks/"
    "refs/heads/main/manifest.json"
)

#: Where a pack's playable sounds live, per category.
GROUPS = {"keyboard": ("default",), "mouse": ("left", "right")}

#: The licenses an asset may be redistributed under. Anything else is refused.
DISTRIBUTABLE = {"mit", "cc0", "pixabay content license"}

#: The one id no registry entry may claim, because the plugin reserves it for
#: "follow the device's own set".
RESERVED_ID = "auto"

ROOT = Path(__file__).resolve().parent
PACKS_DIRECTORY = ROOT / "packs"
RUST_SOURCE = ROOT.parent / "src" / "packs.rs"


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url) as response:  # noqa: S310 - the registry is fixed
        return response.read()


def slug(text: str) -> str:
    return re.sub(r"-+", "-", re.sub(r"[^a-z0-9]+", "-", text.lower())).strip("-")


def unique(candidates: list[str], taken: set[str]) -> str:
    for candidate in candidates:
        if candidate and candidate not in taken and candidate != RESERVED_ID:
            return candidate
    raise SystemExit(f"no unique name among {candidates}")


@dataclass
class Staged:
    id: str
    category: str
    label: str
    entry: dict
    folder: Path
    down: list[str]
    up: list[str]


def playable_names(category: str, config: dict) -> tuple[list[str], list[str]]:
    down: list[str] = []
    up: list[str] = []
    for group in GROUPS[category]:
        sounds = config["sounds"].get(group, {})
        down += sounds.get("down", [])
        up += sounds.get("up", [])
    return sorted(set(down)), sorted(set(up))


def rust_sound_list(category: str, pack_id: str, names: list[str]) -> str:
    """The Rust expression for one set's sounds, embedded from the resources."""
    entries = ",\n".join(
        "            include_bytes!(concat!(\n"
        '                env!("CARGO_MANIFEST_DIR"),\n'
        f'                "/resources/packs/{category}/{pack_id}/{name}"\n'
        "            ))"
        for name in names
    )
    if not entries:
        return " &[]"
    return " &[\n" + entries + ",\n        ]"


def main() -> None:
    manifest = json.loads(fetch(REGISTRY_MANIFEST))
    work = Path(tempfile.mkdtemp(prefix="bongocat-button-sounds-"))
    try:
        staged: list[Staged] = []
        skipped: list[str] = []
        taken_ids: set[str] = set()
        for category, entries in manifest["soundpacks"].items():
            for entry in entries:
                metadata = entry["metadata"]
                license_type = entry["license"]["type"]
                if license_type.lower() not in DISTRIBUTABLE:
                    skipped.append(
                        f"{metadata['name']} ({metadata['brand']}): {license_type}"
                    )
                    continue
                download = work / f"{entry['id']}.zip"
                download.write_bytes(fetch(entry["download"]["url"]))
                folder = work / entry["id"]
                with zipfile.ZipFile(download) as source:
                    source.extractall(folder)
                config = json.loads((folder / "config.json").read_text(encoding="utf-8"))
                down, up = playable_names(category, config)
                if not down:
                    skipped.append(f"{metadata['name']}: no playable sounds")
                    continue
                pack_id = unique(
                    [
                        slug(metadata["name"]),
                        slug(f"{metadata['brand']}-{metadata['name']}"),
                        slug(f"{metadata['author']}-{metadata['brand']}-{metadata['name']}"),
                    ],
                    taken_ids,
                )
                taken_ids.add(pack_id)
                staged.append(Staged(pack_id, category, "", entry, folder, down, up))

        # A label has to name one set on its own, because it is all a row of a
        # dropdown shows: two sets both called "Unknown" would be
        # indistinguishable there.
        bases = [
            f"{pack.entry['metadata']['name']} · {pack.entry['metadata']['brand']}"
            for pack in staged
        ]
        for pack, base in zip(staged, bases):
            pack.label = (
                base
                if bases.count(base) == 1
                else f"{base} · {pack.entry['metadata']['author']}"
            )

        if PACKS_DIRECTORY.exists():
            shutil.rmtree(PACKS_DIRECTORY)

        index = {
            "registry": REGISTRY_MANIFEST,
            "registry_version": manifest["version"],
            "registry_updated": manifest["lastUpdated"],
            "policy": (
                "Only soundpacks whose license permits redistribution are carried, "
                "and each keeps only the groups this plugin can address: the "
                "keyboard `default` group and the mouse left/right groups."
            ),
            "packs": [],
        }
        rust = [
            "//! The sound sets this plugin carries.",
            "//!",
            "//! Generated by `resources/fetch_packs.py`; do not edit by hand. Every",
            "//! sound below is a copy of one Thock soundpack recording, and",
            "//! `resources/index.json` records where each byte came from.",
            "",
            "use crate::PackCategory;",
            "",
            "/// One embedded sound set.",
            "pub struct PackAsset {",
            "    pub id: &'static str,",
            "    pub category: PackCategory,",
            "    pub label: &'static str,",
            "    pub brand: &'static str,",
            "    /// Whether the set carries a release sound at all.",
            "    pub supports_key_up: bool,",
            "    pub down: &'static [&'static [u8]],",
            "    pub up: &'static [&'static [u8]],",
            "}",
            "",
            "pub static PACKS: &[PackAsset] = &[",
        ]
        audio_bytes = 0
        for pack in staged:
            destination = PACKS_DIRECTORY / pack.category / pack.id
            destination.mkdir(parents=True, exist_ok=True)
            files = {}
            for name in [*pack.down, *pack.up]:
                data = (pack.folder / name).read_bytes()
                (destination / name).write_bytes(data)
                audio_bytes += len(data)
                files[name] = hashlib.sha256(data).hexdigest()
            metadata = pack.entry["metadata"]
            index["packs"].append(
                {
                    "id": pack.id,
                    "category": pack.category,
                    "label": pack.label,
                    "name": metadata["name"],
                    "brand": metadata["brand"],
                    "author": metadata["author"],
                    "license": pack.entry["license"],
                    "source": pack.entry["download"]["url"],
                    "files": files,
                }
            )
            rust += [
                "    PackAsset {",
                f'        id: "{pack.id}",',
                f"        category: PackCategory::{pack.category.capitalize()},",
                f'        label: "{pack.label}",',
                f'        brand: {json.dumps(metadata["brand"], ensure_ascii=False)},',
                f"        supports_key_up: {str(bool(pack.up)).lower()},",
                f"        down:{rust_sound_list(pack.category, pack.id, pack.down)},",
                f"        up:{rust_sound_list(pack.category, pack.id, pack.up)},",
                "    },",
            ]
        rust += ["];", ""]

        (ROOT / "index.json").write_text(
            json.dumps(index, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        RUST_SOURCE.write_text("\n".join(rust), encoding="utf-8")
        print(f"wrote {len(staged)} sets, {audio_bytes / 1048576:.2f} MiB of audio")
        for line in skipped:
            print(f"refused: {line}")
    finally:
        shutil.rmtree(work)


if __name__ == "__main__":
    main()
