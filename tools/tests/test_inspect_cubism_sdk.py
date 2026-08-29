from __future__ import annotations

import hashlib
import importlib.util
import stat
import sys
import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path


SCRIPT_PATH = Path(__file__).resolve().parents[1] / "inspect-cubism-sdk.py"
SPEC = importlib.util.spec_from_file_location("inspect_cubism_sdk", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
SDK_INSPECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SDK_INSPECTOR
SPEC.loader.exec_module(SDK_INSPECTOR)


ROOT = "CubismSdkForNative-5-r.5/"
CORE = f"{ROOT}Core/"
FRAMEWORK_SOURCE_CONTENTS = {
    f"{ROOT}Framework/src/CubismModelSettingJson.cpp": b"synthetic model setting source\n",
    f"{ROOT}Framework/src/Motion/CubismMotion.cpp": b"synthetic motion source\n",
}
FRAMEWORK_SOURCE_BLOBS = {
    path.removeprefix(ROOT): SDK_INSPECTOR.git_blob_sha(data)
    for path, data in FRAMEWORK_SOURCE_CONTENTS.items()
}


def valid_entries() -> dict[str, bytes]:
    redistributable = "\n".join(
        [
            "The following is a list of files available for redistribution:",
            *[
                f"- {path}"
                for artifacts in SDK_INSPECTOR.TARGET_ARTIFACTS.values()
                for path in artifacts.values()
            ],
            "",
        ]
    ).encode()
    return {
        f"{ROOT}Framework/CHANGELOG.md": b"# Changelog\n\n## [5-r.5] - 2026-04-02\n",
        f"{CORE}include/Live2DCubismCore.h": b"/* synthetic test header */\n",
        f"{CORE}CHANGELOG.md": b"## 2026-01-08\n\n* Upgrade Core version to 06.00.0001.\n",
        f"{CORE}LICENSE.md": b"synthetic proprietary license marker\n",
        f"{CORE}README.md": b"synthetic readme\n",
        f"{CORE}RedistributableFiles.txt": redistributable,
        f"{CORE}dll/windows/x86_64/Live2DCubismCore.dll": b"x64-dll",
        f"{CORE}dll/windows/x86_64/Live2DCubismCore.lib": b"x64-import-lib",
        f"{CORE}dll/windows/x86/Live2DCubismCore.dll": b"x86-dll",
        f"{CORE}dll/windows/x86/Live2DCubismCore.lib": b"x86-import-lib",
        f"{CORE}lib/macos/arm64/libLive2DCubismCore.a": b"mac-arm64-static",
        f"{CORE}lib/macos/x86_64/libLive2DCubismCore.a": b"mac-x64-static",
        **FRAMEWORK_SOURCE_CONTENTS,
    }


def write_zip(path: Path, entries: dict[str, bytes]) -> None:
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in entries.items():
            archive.writestr(name, data)


def inspect(path: Path, expected_hash: str | None = None) -> dict[str, object]:
    return SDK_INSPECTOR.inspect_archive(
        path,
        expected_hash,
        framework_source_blobs=FRAMEWORK_SOURCE_BLOBS,
    )


class CubismSdkInspectorTests(unittest.TestCase):
    def test_valid_archive_reports_required_targets_and_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "CubismSdkForNative-5-r.5.zip"
            write_zip(path, valid_entries())

            expected_hash = hashlib.sha256(path.read_bytes()).hexdigest()
            report = inspect(path, expected_hash)

            self.assertTrue(report["verified"])
            self.assertEqual(report["archive"]["sha256"], expected_hash)
            self.assertEqual(report["sdk"]["release"], "5-r.5")
            self.assertEqual(report["sdk"]["core_version"], "06.00.0001")
            self.assertEqual(
                report["targets"]["aarch64-pc-windows-msvc"]["status"],
                "unsupported_by_r5",
            )
            self.assertEqual(
                report["targets"]["x86_64-pc-windows-msvc"]["status"],
                "present",
            )

    def test_expected_archive_hash_is_enforced(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            write_zip(path, valid_entries())

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "SHA-256 mismatch"
            ):
                inspect(path, "0" * 64)

    def test_path_traversal_is_rejected_before_artifact_inspection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            entries = valid_entries()
            entries["../outside.txt"] = b"must not escape"
            write_zip(path, entries)

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "not normalized"
            ):
                inspect(path)

    def test_case_colliding_paths_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            entries = valid_entries()
            entries[f"{CORE}readme.md"] = b"case collision"
            write_zip(path, entries)

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "non-portable paths"
            ):
                inspect(path)

    def test_symbolic_link_entry_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            write_zip(path, valid_entries())
            link = zipfile.ZipInfo(f"{ROOT}link")
            link.create_system = 3
            link.external_attr = (stat.S_IFLNK | 0o777) << 16
            with zipfile.ZipFile(path, "a") as archive:
                archive.writestr(link, "Core/include/Live2DCubismCore.h")

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "symbolic link"
            ):
                inspect(path)

    def test_duplicate_path_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            write_zip(path, valid_entries())
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                with zipfile.ZipFile(path, "a") as archive:
                    archive.writestr(f"{CORE}README.md", b"duplicate")

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "duplicate"
            ):
                inspect(path)

    def test_missing_required_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            entries = valid_entries()
            del entries[f"{CORE}dll/windows/x86_64/Live2DCubismCore.dll"]
            write_zip(path, entries)

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "windows_x64_dll is missing"
            ):
                inspect(path)

    def test_framework_release_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            entries = valid_entries()
            entries[f"{ROOT}Framework/CHANGELOG.md"] = b"## [5-r.4.1]\n"
            write_zip(path, entries)

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "Framework release mismatch"
            ):
                inspect(path)

    def test_framework_source_blob_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sdk.zip"
            entries = valid_entries()
            source_path = next(iter(FRAMEWORK_SOURCE_CONTENTS))
            entries[source_path] = b"drifted source\n"
            write_zip(path, entries)

            with self.assertRaisesRegex(
                SDK_INSPECTOR.InspectionError, "Framework source blob mismatch"
            ):
                inspect(path)


if __name__ == "__main__":
    unittest.main()
