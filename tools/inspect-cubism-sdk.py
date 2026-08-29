#!/usr/bin/env python3
"""Inspect a locally acquired Cubism Native SDK ZIP without extracting it."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import sys
import unicodedata
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


EXPECTED_SDK_RELEASE = "5-r.5"
EXPECTED_CORE_VERSION = "06.00.0001"
MAX_ENTRIES = 50_000
MAX_MEMBER_BYTES = 256 * 1024 * 1024
MAX_TEXT_BYTES = 4 * 1024 * 1024
MAX_TOTAL_UNCOMPRESSED_BYTES = 2 * 1024 * 1024 * 1024
MAX_COMPRESSION_RATIO = 250

LEGAL_FILES = {
    "core_changelog": "CHANGELOG.md",
    "core_license": "LICENSE.md",
    "core_readme": "README.md",
    "core_redistributable_files": "RedistributableFiles.txt",
}

TARGET_ARTIFACTS = {
    "x86_64-pc-windows-msvc": {
        "windows_x64_dll": "dll/windows/x86_64/Live2DCubismCore.dll",
        "windows_x64_import_library": "dll/windows/x86_64/Live2DCubismCore.lib",
    },
    "i686-pc-windows-msvc": {
        "windows_x86_dll": "dll/windows/x86/Live2DCubismCore.dll",
        "windows_x86_import_library": "dll/windows/x86/Live2DCubismCore.lib",
    },
    "aarch64-apple-darwin": {
        "macos_arm64_static_library": "lib/macos/arm64/libLive2DCubismCore.a",
    },
    "x86_64-apple-darwin": {
        "macos_x64_static_library": "lib/macos/x86_64/libLive2DCubismCore.a",
    },
}

CORE_HEADER = "include/Live2DCubismCore.h"
FRAMEWORK_CHANGELOG_SUFFIX = "Framework/CHANGELOG.md"
FRAMEWORK_TREE_SHA = "a140eec8da452762fcad566329074ad4d1cd6130"
FRAMEWORK_SOURCE_BLOBS = {
    "Framework/src/CubismModelSettingJson.cpp": "8b9fa84d5d74a0882b2d5f20322862606207c6a6",
    "Framework/src/Effect/CubismBreath.cpp": "9312b1f96b25380670856f9cecc3dee33ea9ad02",
    "Framework/src/Effect/CubismEyeBlink.cpp": "7b67806753b76cac1fd053ed899ff761aa0156b4",
    "Framework/src/Effect/CubismPose.cpp": "fcb88823d17466359f87c7e2a88e309fc54b19c4",
    "Framework/src/Motion/CubismExpressionMotion.cpp": "5f79270126c487c38075a853d0081c974b081060",
    "Framework/src/Motion/CubismMotion.cpp": "702f85a1a4057dc695eba47088f9338409937bce",
    "Framework/src/Motion/CubismMotionJson.cpp": "6cd35be1923a26014c5bd155ecad9eccbc9cd1e2",
    "Framework/src/Motion/CubismUpdateScheduler.cpp": "76d967d6a6788165a68e6e34653ead3ba40acee8",
    "Framework/src/Motion/ICubismUpdater.hpp": "00e1b8000c4a9c7263e36f58d2ac9ea6a8476d4e",
    "Framework/src/Physics/CubismPhysics.cpp": "5cb44241c1f3faeb6dcac7463c0c18eab9dac431",
    "Framework/src/Physics/CubismPhysicsJson.cpp": "8cfdc05564e24ece369035fdf90fb546b94d90c6",
    "Framework/src/Rendering/CubismRenderer.cpp": "ce008f9148b1fd591d077ab90a963da9431ac08c",
    "Framework/src/Rendering/D3D11/CubismRenderer_D3D11.cpp": "917b46ba352f4e80566c07369baba3d703ec54fb",
    "Framework/src/Rendering/D3D11/Shaders/CubismEffect.fx": "bbaca13cbbcfb9b184e6e8a5e63f40e99619f217",
    "Framework/src/Rendering/Metal/CubismRenderer_Metal.mm": "d46eddfb748669a55e6c47ea22bfba5774ec4504",
    "Framework/src/Rendering/Metal/Shaders/MetalShaders.metal": "696adec0e2e38e1fa83d499f1369c388bab1576a",
}


class InspectionError(ValueError):
    """Raised when an SDK archive does not satisfy the inspection contract."""


@dataclass(frozen=True)
class LocatedMember:
    role: str
    relative_path: str
    info: zipfile.ZipInfo


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_blob_sha(data: bytes) -> str:
    header = f"blob {len(data)}\0".encode("ascii")
    return hashlib.sha1(header + data, usedforsecurity=False).hexdigest()


def normalized_archive_name(name: str) -> str:
    if not name or "\x00" in name:
        raise InspectionError("archive contains an empty or NUL-containing path")
    if "\\" in name:
        raise InspectionError(f"archive path uses a backslash: {name!r}")
    if name.startswith("/") or re.match(r"^[A-Za-z]:", name):
        raise InspectionError(f"archive contains an absolute path: {name!r}")

    parts = name.split("/")
    if parts[-1] == "":
        parts = parts[:-1]
    if not parts or any(part in {"", ".", ".."} for part in parts):
        raise InspectionError(f"archive path is not normalized: {name!r}")

    normalized = PurePosixPath(*parts).as_posix()
    return normalized


def validate_archive_entries(infos: list[zipfile.ZipInfo]) -> dict[str, zipfile.ZipInfo]:
    if len(infos) > MAX_ENTRIES:
        raise InspectionError(
            f"archive has {len(infos)} entries; limit is {MAX_ENTRIES}"
        )

    entries: dict[str, zipfile.ZipInfo] = {}
    portable_names: dict[str, str] = {}
    total_uncompressed = 0

    for info in infos:
        name = normalized_archive_name(info.filename)
        portable_name = unicodedata.normalize("NFC", name).casefold()
        previous = portable_names.get(portable_name)
        if previous is not None:
            raise InspectionError(
                f"archive contains duplicate or non-portable paths: {previous!r}, {name!r}"
            )
        portable_names[portable_name] = name

        unix_mode = info.external_attr >> 16
        if stat.S_IFMT(unix_mode) == stat.S_IFLNK:
            raise InspectionError(f"archive contains a symbolic link: {name!r}")
        if info.flag_bits & 0x1:
            raise InspectionError(f"archive contains an encrypted entry: {name!r}")

        if info.is_dir():
            continue
        if info.file_size > MAX_MEMBER_BYTES:
            raise InspectionError(
                f"archive entry exceeds {MAX_MEMBER_BYTES} bytes: {name!r}"
            )
        total_uncompressed += info.file_size
        if total_uncompressed > MAX_TOTAL_UNCOMPRESSED_BYTES:
            raise InspectionError(
                "archive uncompressed size exceeds "
                f"{MAX_TOTAL_UNCOMPRESSED_BYTES} bytes"
            )
        if info.file_size > 1024 * 1024:
            ratio = info.file_size / max(info.compress_size, 1)
            if ratio > MAX_COMPRESSION_RATIO:
                raise InspectionError(
                    f"archive entry compression ratio is suspicious ({ratio:.1f}): {name!r}"
                )
        entries[name] = info

    return entries


def find_unique_suffix(
    entries: dict[str, zipfile.ZipInfo], suffix: str, role: str
) -> zipfile.ZipInfo:
    matches = [info for name, info in entries.items() if name == suffix or name.endswith(f"/{suffix}")]
    if not matches:
        raise InspectionError(f"required {role} is missing: */{suffix}")
    if len(matches) > 1:
        paths = ", ".join(sorted(info.filename for info in matches))
        raise InspectionError(f"required {role} is ambiguous: {paths}")
    return matches[0]


def hash_member(archive: zipfile.ZipFile, info: zipfile.ZipInfo) -> str:
    digest = hashlib.sha256()
    bytes_read = 0
    with archive.open(info, "r") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            bytes_read += len(chunk)
            if bytes_read > MAX_MEMBER_BYTES:
                raise InspectionError(f"entry is too large to inspect: {info.filename!r}")
            digest.update(chunk)
    if bytes_read != info.file_size:
        raise InspectionError(f"entry size changed while reading: {info.filename!r}")
    return digest.hexdigest()


def member_bytes(archive: zipfile.ZipFile, info: zipfile.ZipInfo) -> bytes:
    if info.file_size > MAX_TEXT_BYTES:
        raise InspectionError(f"text entry is too large to inspect: {info.filename!r}")
    with archive.open(info, "r") as source:
        data = source.read(MAX_TEXT_BYTES + 1)
    if len(data) != info.file_size:
        raise InspectionError(f"entry size changed while reading: {info.filename!r}")
    if len(data) > MAX_TEXT_BYTES:
        raise InspectionError(f"text entry is too large to inspect: {info.filename!r}")
    return data


def member_report(
    archive: zipfile.ZipFile, member: LocatedMember
) -> dict[str, str | int]:
    return {
        "role": member.role,
        "path": normalized_archive_name(member.info.filename),
        "relative_path": member.relative_path,
        "size": member.info.file_size,
        "sha256": hash_member(archive, member.info),
    }


def decode_text(archive: zipfile.ZipFile, info: zipfile.ZipInfo, role: str) -> str:
    try:
        return member_bytes(archive, info).decode("utf-8")
    except UnicodeDecodeError as exc:
        raise InspectionError(f"{role} is not UTF-8: {info.filename!r}") from exc


def find_core_root(entries: dict[str, zipfile.ZipInfo]) -> tuple[str, zipfile.ZipInfo]:
    header = find_unique_suffix(entries, CORE_HEADER, "Core header")
    header_name = normalized_archive_name(header.filename)
    core_root = header_name[: -len(CORE_HEADER)]
    return core_root, header


def core_member(
    entries: dict[str, zipfile.ZipInfo], core_root: str, relative_path: str, role: str
) -> zipfile.ZipInfo:
    path = f"{core_root}{relative_path}"
    info = entries.get(path)
    if info is None:
        raise InspectionError(f"required {role} is missing: {path}")
    return info


def detect_framework_release(
    archive: zipfile.ZipFile, entries: dict[str, zipfile.ZipInfo]
) -> str:
    info = find_unique_suffix(entries, FRAMEWORK_CHANGELOG_SUFFIX, "Framework changelog")
    changelog = decode_text(archive, info, "Framework changelog")
    match = re.search(r"^## \[([^]]+)]", changelog, re.MULTILINE)
    if match is None:
        raise InspectionError("Framework changelog does not contain a release heading")
    return match.group(1)


def validate_framework_sources(
    archive: zipfile.ZipFile,
    entries: dict[str, zipfile.ZipInfo],
    expected_blobs: dict[str, str],
) -> list[dict[str, str | int]]:
    reports = []
    for source_path, expected_blob in sorted(expected_blobs.items()):
        info = find_unique_suffix(entries, source_path, f"Framework source {source_path}")
        data = member_bytes(archive, info)
        actual_blob = git_blob_sha(data)
        if actual_blob != expected_blob:
            raise InspectionError(
                f"Framework source blob mismatch for {source_path}: "
                f"expected {expected_blob}, got {actual_blob}"
            )
        reports.append(
            {
                "path": normalized_archive_name(info.filename),
                "relative_path": source_path,
                "size": info.file_size,
                "git_blob_sha": actual_blob,
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        )
    return reports


def detect_core_version(core_changelog: str) -> str:
    match = re.search(
        r"Upgrade Core version to [`]?([0-9]{2}\.[0-9]{2}\.[0-9]{4})[`]?",
        core_changelog,
    )
    if match is None:
        raise InspectionError("Core changelog does not identify a Core version")
    return match.group(1)


def validate_redistributable_list(text: str) -> None:
    listed = {
        line.removeprefix("- ").strip()
        for line in text.splitlines()
        if line.startswith("- ")
    }
    required = {
        relative
        for artifacts in TARGET_ARTIFACTS.values()
        for relative in artifacts.values()
    }
    missing = sorted(required - listed)
    if missing:
        raise InspectionError(
            "RedistributableFiles.txt does not list required artifacts: "
            + ", ".join(missing)
        )


def inspect_archive(
    path: Path,
    expected_sha256: str | None = None,
    framework_source_blobs: dict[str, str] | None = None,
) -> dict[str, object]:
    if not path.is_file():
        raise InspectionError(f"SDK ZIP does not exist or is not a file: {path}")
    if not zipfile.is_zipfile(path):
        raise InspectionError(f"input is not a valid ZIP archive: {path}")

    archive_sha256 = sha256_file(path)
    if expected_sha256 is not None and archive_sha256 != expected_sha256.lower():
        raise InspectionError(
            f"archive SHA-256 mismatch: expected {expected_sha256.lower()}, got {archive_sha256}"
        )

    with zipfile.ZipFile(path, "r") as archive:
        infos = archive.infolist()
        entries = validate_archive_entries(infos)
        core_root, header = find_core_root(entries)

        legal_members: list[LocatedMember] = []
        for role, relative_path in LEGAL_FILES.items():
            info = core_member(entries, core_root, relative_path, role)
            legal_members.append(LocatedMember(role, relative_path, info))

        target_members: dict[str, list[LocatedMember]] = {}
        for target, artifacts in TARGET_ARTIFACTS.items():
            target_members[target] = [
                LocatedMember(
                    role,
                    relative_path,
                    core_member(entries, core_root, relative_path, role),
                )
                for role, relative_path in artifacts.items()
            ]

        unexpected_arm64 = [
            name
            for name in entries
            if name.startswith(f"{core_root}dll/windows/arm64/")
            or name.startswith(f"{core_root}lib/windows/arm64/")
        ]
        if unexpected_arm64:
            raise InspectionError(
                "R5 archive unexpectedly contains desktop Windows ARM64 artifacts; "
                "review the target matrix before continuing"
            )

        framework_release = detect_framework_release(archive, entries)
        if framework_release != EXPECTED_SDK_RELEASE:
            raise InspectionError(
                f"Framework release mismatch: expected {EXPECTED_SDK_RELEASE}, "
                f"got {framework_release}"
            )
        expected_framework_blobs = (
            FRAMEWORK_SOURCE_BLOBS
            if framework_source_blobs is None
            else framework_source_blobs
        )
        framework_sources = validate_framework_sources(
            archive, entries, expected_framework_blobs
        )

        core_changelog_info = next(
            member.info for member in legal_members if member.role == "core_changelog"
        )
        core_changelog = decode_text(archive, core_changelog_info, "Core changelog")
        core_version = detect_core_version(core_changelog)
        if core_version != EXPECTED_CORE_VERSION:
            raise InspectionError(
                f"Core version mismatch: expected {EXPECTED_CORE_VERSION}, got {core_version}"
            )

        redistributable_info = next(
            member.info
            for member in legal_members
            if member.role == "core_redistributable_files"
        )
        validate_redistributable_list(
            decode_text(archive, redistributable_info, "RedistributableFiles.txt")
        )

        header_member = LocatedMember("core_header", CORE_HEADER, header)
        target_reports = {
            target: {
                "status": "present",
                "artifacts": [member_report(archive, member) for member in members],
            }
            for target, members in target_members.items()
        }
        target_reports["aarch64-pc-windows-msvc"] = {
            "status": "unsupported_by_r5",
            "artifacts": [],
        }

        total_uncompressed = sum(
            info.file_size for info in infos if not info.is_dir()
        )
        return {
            "schema_version": 1,
            "verified": True,
            "archive": {
                "filename": path.name,
                "size": path.stat().st_size,
                "sha256": archive_sha256,
                "entry_count": len(infos),
                "total_uncompressed_bytes": total_uncompressed,
            },
            "sdk": {
                "release": framework_release,
                "core_version": core_version,
                "core_root": core_root,
                "framework_tree_sha": FRAMEWORK_TREE_SHA,
            },
            "framework_sources": framework_sources,
            "core_header": member_report(archive, header_member),
            "legal_files": [
                member_report(archive, member) for member in legal_members
            ],
            "targets": target_reports,
        }


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Validate and hash a locally acquired Cubism 5 SDK for Native R5 ZIP. "
            "The archive is never extracted and no network request is made."
        )
    )
    parser.add_argument("sdk_zip", type=Path, help="path to CubismSdkForNative-5-r.5.zip")
    parser.add_argument(
        "--expected-sha256",
        type=str.lower,
        help="reject the archive unless its SHA-256 matches this value",
    )
    args = parser.parse_args(argv)
    if args.expected_sha256 is not None and not re.fullmatch(
        r"[0-9a-f]{64}", args.expected_sha256
    ):
        parser.error("--expected-sha256 must be exactly 64 hexadecimal characters")
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        report = inspect_archive(args.sdk_zip, args.expected_sha256)
    except (InspectionError, OSError, zipfile.BadZipFile) as exc:
        print(f"Cubism SDK inspection failed: {exc}", file=sys.stderr)
        return 1
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
