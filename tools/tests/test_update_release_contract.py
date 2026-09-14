"""Pin the release identity that `bongocat-update` and the packaging pipeline share.

`cargo-packager-updater` reads that identity out of compile-time constants in
`crates/bongocat-update/src/runtime.rs`:

* `RELEASE_REPOSITORY_OWNER` / `RELEASE_REPOSITORY_NAME` become the manifest URL the
  updater requests, so they have to name the repository that publishes the releases.
* `RELEASE_MANIFEST_NAME` is the shared manifest asset that URL ends in.
* `UpdateTargetTriple::manifest_platform` produces the `<os>-<arch>` keys inside that
  manifest, which the library looks this host up under.
* `RELEASE_BINARY_NAME` and `RELEASE_BUNDLE_NAME` are the executable and bundle the
  packaging pipeline builds and the updater installs.

`crates/bongocat-packaging` writes the payload and the manifest the updater reads, so
both sides have to spell the same names. None of it is checked by the compiler, and a
mismatch only shows up the first time a real update runs. These tests fail loudly
instead.

The manifest *shape* is not pinned here: `crates/bongocat-packaging`'s merge test feeds
the manifest it writes to the update library's own reader type, which is a stronger
check than matching its JSON keys from outside.
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
PACKAGER = ROOT / "crates" / "bongocat-packaging" / "src" / "main.rs"

EXPECTED_PLATFORM_KEYS = ["macos-aarch64", "macos-x86_64", "windows-x86_64"]


def read(path):
    return path.read_text(encoding="utf-8")


def section(source, header):
    """Return the body of a TOML section, excluding the following header."""
    match = re.search(rf"\[{re.escape(header)}\](.*?)\n\[", source, re.DOTALL)
    if match is None:
        raise AssertionError(f"section [{header}] not found")
    return match.group(1)


def rust_string_constant(source, name):
    match = re.search(rf'const {re.escape(name)}: &str = "([^"]*)";', source)
    if match is None:
        raise AssertionError(f"{name} is not declared as a string constant")
    return match.group(1)


def runtime_constant(name):
    return rust_string_constant(read(RUNTIME), name)


def packaging_constant(name):
    return rust_string_constant(read(PACKAGER), name)


def manifest_platform_keys(source, type_name):
    """The `Self::Variant => "key"` pairs of the type's `manifest_platform`."""
    match = re.search(
        r"fn manifest_platform\(self\).*?match self \{(.*?)\n        \}",
        source,
        re.DOTALL,
    )
    if match is None:
        raise AssertionError(f"{type_name}::manifest_platform not found")
    keys = sorted(set(re.findall(r'Self::\w+ => "([^"]+)"', match.group(1))))
    if not keys:
        raise AssertionError(f"{type_name}::manifest_platform maps no variants")
    return keys


class UpdateReleaseIdentityTests(unittest.TestCase):
    def test_release_executable_name_matches_the_shipped_executable(self):
        binary = runtime_constant("RELEASE_BINARY_NAME")

        app_name = re.search(
            r'^name = "([^"]+)"',
            section(read(APP_MANIFEST), "package"),
            re.MULTILINE,
        )
        self.assertIsNotNone(app_name, "bongocat-app must declare a package name")
        self.assertEqual(
            binary,
            app_name.group(1),
            "RELEASE_BINARY_NAME must name the executable the packaging pipeline builds",
        )

        self.assertIn(
            f'const APPLICATION_BINARY: &str = "{binary}";',
            read(PACKAGER),
            "the packaging pipeline must build the executable the release identity names",
        )

    def test_macos_bundle_name_matches_the_packaged_app(self):
        bundle = runtime_constant("RELEASE_BUNDLE_NAME")

        product_name = re.search(r'const PRODUCT_NAME: &str = "([^"]+)";', read(PACKAGER))
        self.assertIsNotNone(
            product_name, "the packaging tool must declare the product name"
        )
        self.assertEqual(
            bundle,
            f"{product_name.group(1)}.app",
            "RELEASE_BUNDLE_NAME must be the .app directory the packaging pipeline builds, "
            "because cargo-packager names the bundle after the product name",
        )

    def test_repository_matches_the_workspace_manifest(self):
        repository = re.search(
            r'repository = "([^"]+)"',
            section(read(WORKSPACE_MANIFEST), "workspace.package"),
        )
        self.assertIsNotNone(repository, "the workspace must declare a repository")
        remote = re.search(r"^https://github\.com/([^/]+)/([^/]+)$", repository.group(1))
        self.assertIsNotNone(remote, "the repository must be a github.com URL")

        self.assertEqual(runtime_constant("RELEASE_REPOSITORY_OWNER"), remote.group(1))
        self.assertEqual(runtime_constant("RELEASE_REPOSITORY_NAME"), remote.group(2))

        # The packaging tool writes the manifest's asset URLs, so it has to name the
        # same repository.
        self.assertEqual(
            packaging_constant("RELEASE_REPOSITORY_URL"),
            repository.group(1).removesuffix(".git"),
            "the packaging tool must publish manifest URLs for the same repository",
        )

    def test_release_targets_match_the_dependency_policy_matrix(self):
        source = read(RELEASE)
        block = re.search(r"impl UpdateTargetTriple \{(.*?)\n\}", source, re.DOTALL)
        self.assertIsNotNone(block, "UpdateTargetTriple must keep an inherent impl")

        declared = sorted(set(re.findall(r'Self::\w+ => "([^"]+)"', block.group(1))))
        self.assertTrue(declared, "UpdateTargetTriple::as_str must map every variant")

        policy = sorted(
            re.findall(
                r'"([a-z0-9_]+-[a-z0-9_-]+)"',
                section(read(DENY), "graph"),
            )
        )
        self.assertEqual(
            declared,
            policy,
            "the updatable targets and the audited dependency-policy targets must agree",
        )

    def test_manifest_platform_keys_agree_with_the_packaging_tool(self):
        runtime_keys = manifest_platform_keys(read(RELEASE), "UpdateTargetTriple")
        packaging_keys = manifest_platform_keys(read(PACKAGER), "ReleaseTarget")

        self.assertEqual(
            runtime_keys,
            packaging_keys,
            "the runtime requests a manifest under the platform key the packaging tool "
            "writes; a drift here means the updater asks for an asset that never exists",
        )
        self.assertEqual(
            runtime_keys,
            EXPECTED_PLATFORM_KEYS,
            "the keys must use cargo-packager-updater's `<os>-<arch>` spelling",
        )

    def test_manifest_name_agrees_with_the_packaging_tool(self):
        """The runtime requests one shared asset, so both sides have to name it alike.

        The packaging tool publishes it and the runtime requests it, and neither crate
        depends on the other, so nothing but this test connects the two constants.
        """
        name = runtime_constant("RELEASE_MANIFEST_NAME")
        self.assertEqual(
            name,
            packaging_constant("UPDATE_MANIFEST_NAME"),
            "the runtime and the packaging tool must agree on the manifest asset name",
        )
        self.assertEqual(
            name,
            "latest.json",
            "the manifest asset name is part of the published release contract",
        )

    def test_manifest_fragments_are_named_after_the_platform_keys(self):
        """The merge reads the platform key off a fragment's file name.

        So the fragment name has to be `<platform key>` plus the tool's fragment
        suffix, and every key the runtime declares has to be a name the tool can write
        and the merge can recognise.
        """
        suffix = packaging_constant("UPDATE_FRAGMENT_SUFFIX")
        self.assertTrue(
            suffix.startswith("."),
            f"the fragment suffix must be an extension, got {suffix!r}",
        )

        runtime_keys = manifest_platform_keys(read(RELEASE), "UpdateTargetTriple")
        packaging_keys = manifest_platform_keys(read(PACKAGER), "ReleaseTarget")
        for key in packaging_keys:
            with self.subTest(key=key):
                self.assertIn(
                    key,
                    runtime_keys,
                    "the tool can only write a fragment for a key the runtime knows",
                )
                self.assertNotIn(
                    "/",
                    key,
                    "a fragment name is a file name, so a key cannot contain a separator",
                )


if __name__ == "__main__":
    unittest.main()
