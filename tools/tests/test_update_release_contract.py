"""Pin the release identity that `bongocat-update` and the packaging scripts must agree on.

`self_update` reads the release identity out of compile-time constants in
`crates/bongocat-update/src/runtime.rs`:

* `RELEASE_BINARY_NAME` becomes the single path it extracts from a Windows
  archive (`{name}{EXE_SUFFIX}`), so it has to equal the executable the Windows
  pipeline actually ships.
* `RELEASE_BUNDLE_NAME` is the macOS directory it swaps in bundle mode, so it
  has to equal the `.app` the macOS pipeline actually produces.
* the repository owner/name decide which GitHub releases are queried.

None of that is checked by the compiler, and a mismatch only shows up the first
time a real update runs. These tests fail loudly instead.
"""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RUNTIME = ROOT / "crates" / "bongocat-update" / "src" / "runtime.rs"
RELEASE = ROOT / "crates" / "bongocat-update" / "src" / "release.rs"
APP_MANIFEST = ROOT / "crates" / "bongocat-app" / "Cargo.toml"
WORKSPACE_MANIFEST = ROOT / "Cargo.toml"
DENY = ROOT / "deny.toml"
WINDOWS_BUILD = ROOT / "scripts" / "build-windows.ps1"
MACOS_PACKAGE = ROOT / "scripts" / "package-macos.sh"


def section(source, header):
    """Return the body of a TOML section, excluding the following header."""
    match = re.search(rf"\[{re.escape(header)}\](.*?)\n\[", source, re.DOTALL)
    if match is None:
        raise AssertionError(f"section [{header}] not found")
    return match.group(1)


def runtime_constant(name):
    source = RUNTIME.read_text(encoding="utf-8")
    match = re.search(rf'pub const {name}: &str = "([^"]+)";', source)
    if match is None:
        raise AssertionError(f"{name} is not declared in {RUNTIME}")
    return match.group(1)


class UpdateReleaseIdentityTests(unittest.TestCase):
    def test_windows_binary_name_matches_the_shipped_executable(self):
        binary = runtime_constant("RELEASE_BINARY_NAME")

        app_name = re.search(
            r'^name = "([^"]+)"',
            section(APP_MANIFEST.read_text(encoding="utf-8"), "package"),
            re.MULTILINE,
        )
        self.assertIsNotNone(app_name, "bongocat-app must declare a package name")
        self.assertEqual(
            binary,
            app_name.group(1),
            "RELEASE_BINARY_NAME must be the bongocat-app executable name, because "
            "self_update derives the Windows archive path from it",
        )

        windows_build = WINDOWS_BUILD.read_text(encoding="utf-8")
        self.assertIn(
            f"{binary}.exe",
            windows_build,
            "the Windows build must ship the executable self_update looks for",
        )

    def test_macos_bundle_name_matches_the_packaged_app(self):
        bundle = runtime_constant("RELEASE_BUNDLE_NAME")

        macos_package = MACOS_PACKAGE.read_text(encoding="utf-8")
        packaged = re.search(r"target/package/(\S+\.app)", macos_package)
        self.assertIsNotNone(packaged, "package-macos.sh must build a .app")
        self.assertEqual(
            bundle,
            packaged.group(1),
            "RELEASE_BUNDLE_NAME must be the .app directory package-macos.sh builds",
        )

    def test_repository_matches_the_workspace_manifest(self):
        repository = re.search(
            r'repository = "([^"]+)"',
            section(WORKSPACE_MANIFEST.read_text(encoding="utf-8"), "workspace.package"),
        )
        self.assertIsNotNone(repository, "the workspace must declare a repository")
        remote = re.search(r"^https://github\.com/([^/]+)/([^/]+)$", repository.group(1))
        self.assertIsNotNone(remote, "the repository must be a github.com URL")

        self.assertEqual(runtime_constant("RELEASE_REPOSITORY_OWNER"), remote.group(1))
        self.assertEqual(runtime_constant("RELEASE_REPOSITORY_NAME"), remote.group(2))

    def test_release_targets_match_the_dependency_policy_matrix(self):
        source = RELEASE.read_text(encoding="utf-8")
        block = re.search(r"impl UpdateTargetTriple \{(.*?)\n\}", source, re.DOTALL)
        self.assertIsNotNone(block, "UpdateTargetTriple must keep an inherent impl")

        declared = sorted(set(re.findall(r'Self::\w+ => "([^"]+)"', block.group(1))))
        self.assertTrue(declared, "UpdateTargetTriple::as_str must map every variant")

        policy = sorted(
            re.findall(
                r'"([a-z0-9_]+-[a-z0-9_-]+)"',
                section(DENY.read_text(encoding="utf-8"), "graph"),
            )
        )
        self.assertEqual(
            declared,
            policy,
            "the updatable targets and the audited dependency-policy targets must agree",
        )


if __name__ == "__main__":
    unittest.main()
