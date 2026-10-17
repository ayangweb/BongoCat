"""Keep shipped BongoCat product metadata derived from one workspace value."""

import json
import plistlib
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKSPACE_MANIFEST = ROOT / "Cargo.toml"
LOCKFILE = ROOT / "Cargo.lock"
MACOS_INFO = ROOT / "macos" / "Info.plist"
PACKAGER = ROOT / "crates" / "bongocat-packaging" / "src" / "main.rs"
PACKAGER_MANIFEST = ROOT / "crates" / "bongocat-packaging" / "Cargo.toml"
WINDOWS_RESOURCE = ROOT / "crates" / "bongocat-app" / "windows" / "bongocat-app.rc"
APP_BUILD_SCRIPT = ROOT / "crates" / "bongocat-app" / "build.rs"
APP_LIBRARY = ROOT / "crates" / "bongocat-app" / "src" / "lib.rs"
APP_SETTINGS_SNAPSHOT = ROOT / "crates" / "bongocat-app" / "src" / "settings" / "snapshot.rs"
UI_LIBRARY = ROOT / "crates" / "bongocat-ui" / "src" / "lib.rs"
UI_WINDOW_COPY_TESTS = ROOT / "crates" / "bongocat-ui" / "src" / "window" / "tests" / "copy.rs"
UPDATE_RUNTIME_TESTS = ROOT / "crates" / "bongocat-update" / "src" / "runtime" / "tests.rs"
UPDATE_CAPABILITY_TESTS = (
    ROOT / "crates" / "bongocat-update" / "tests" / "release_manifest_capability.rs"
)
PRODUCT_RUNTIME_DOC = ROOT / "docs" / "product-runtime.md"
# Plugin packages are built, versioned and distributed on their own (see
# ADR-0090/ADR-0091): a plugin's version is the one its `plugin.json` publishes,
# the host declares the range it accepts through `app_versions`, and a plugin
# update has to be able to ship without a product release. They are workspace
# members only so one `cargo` invocation builds everything, so the product
# version contract covers the crates under `crates/` and not those under
# `plugins/`.
PLUGIN_MEMBER_PREFIX = "plugins/"


def read(path):
    return path.read_text(encoding="utf-8")


def workspace_package_version():
    source = read(WORKSPACE_MANIFEST)
    section = re.search(
        r"^\[workspace\.package\]\s*\n(?P<body>.*?)(?=^\[|\Z)",
        source,
        re.MULTILINE | re.DOTALL,
    )
    if section is None:
        raise AssertionError("[workspace.package] is missing from Cargo.toml")

    version = re.search(
        r'^version\s*=\s*"([^"]+)"\s*$',
        section.group("body"),
        re.MULTILINE,
    )
    if version is None:
        raise AssertionError("[workspace.package].version is missing from Cargo.toml")
    return version.group(1)


def workspace_members():
    source = read(WORKSPACE_MANIFEST)
    section = re.search(
        r"^\[workspace\]\s*\n(?P<body>.*?)(?=^\[|\Z)",
        source,
        re.MULTILINE | re.DOTALL,
    )
    if section is None:
        raise AssertionError("[workspace] is missing from Cargo.toml")

    members = re.search(
        r"^members\s*=\s*\[(?P<body>.*?)\]",
        section.group("body"),
        re.MULTILINE | re.DOTALL,
    )
    if members is None:
        raise AssertionError("[workspace].members is missing from Cargo.toml")
    return re.findall(r'"([^"]+)"', members.group("body"))


class ProductVersionContractTests(unittest.TestCase):
    def test_workspace_owns_a_valid_product_version(self):
        version = workspace_package_version()
        self.assertRegex(version, r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")

    def test_workspace_members_inherit_the_product_version(self):
        members = [
            member
            for member in workspace_members()
            if not member.startswith(PLUGIN_MEMBER_PREFIX)
        ]
        self.assertTrue(members, "the workspace must contain product crates")

        for member in members:
            with self.subTest(member=member):
                manifest = ROOT / member / "Cargo.toml"
                source = read(manifest)
                self.assertRegex(source, r"(?m)^version\.workspace\s*=\s*true\s*$")
                self.assertNotRegex(source, r"(?m)^version\s*=\s*\"")

    def test_plugin_packages_version_themselves(self):
        plugins = [
            member
            for member in workspace_members()
            if member.startswith(PLUGIN_MEMBER_PREFIX)
        ]
        self.assertTrue(plugins, "the workspace must list the independently built plugins")

        for member in plugins:
            with self.subTest(member=member):
                manifest = read(ROOT / member / "Cargo.toml")
                declared = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', manifest)
                self.assertIsNotNone(
                    declared, f"{member} must declare its own package version"
                )
                # The built binary and the package the center installs have to
                # report the same version: `plugin.json` is what discovery, the
                # catalog and an update comparison read.
                plugin_manifest = json.loads(
                    read(ROOT / member / "plugin.json")
                )
                self.assertEqual(
                    plugin_manifest["metadata"]["version"],
                    declared.group(1),
                    f"{member} must ship the version its package manifest declares",
                )

    def test_lockfile_workspace_packages_match_the_product_version(self):
        expected_version = workspace_package_version()
        source = read(LOCKFILE)
        packages = dict(
            re.findall(
                r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"',
                source,
            )
        )
        plugin_names = {
            re.search(
                r'(?m)^name\s*=\s*"([^"]+)"\s*$', read(ROOT / member / "Cargo.toml")
            ).group(1)
            for member in workspace_members()
            if member.startswith(PLUGIN_MEMBER_PREFIX)
        }
        product_packages = {
            name: version
            for name, version in packages.items()
            if name.startswith("bongocat-")
            and name not in plugin_names
        }

        self.assertTrue(product_packages, "Cargo.lock must contain product packages")
        self.assertEqual(
            set(product_packages.values()),
            {expected_version},
            "every bongocat product package must inherit the product version",
        )

    def test_runtime_and_ui_use_the_compiled_product_version(self):
        self.assertIn(
            'pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");',
            read(APP_LIBRARY),
        )
        self.assertIn(
            "product_version: PRODUCT_VERSION.to_owned()", read(APP_SETTINGS_SNAPSHOT)
        )
        self.assertIn(
            'product_version: env!("CARGO_PKG_VERSION").to_owned()',
            read(UI_LIBRARY),
        )
        copy_tests = read(UI_WINDOW_COPY_TESTS)
        self.assertIn(
            'let product_version = env!("CARGO_PKG_VERSION");', copy_tests
        )
        self.assertIn(
            'format!("Version {product_version}")',
            copy_tests,
        )
        self.assertIn(
            'format!("版本 {product_version}")',
            copy_tests,
        )
        # The About row names the version and not the build environment, so the
        # environment suffix has to stay out of the expectation rather than be
        # pinned by it.
        self.assertNotIn("· Development build", copy_tests)
        self.assertNotIn("· 开发版", copy_tests)

        update_runtime = read(UPDATE_RUNTIME_TESTS)
        self.assertIn('env!("CARGO_PKG_VERSION")', update_runtime)

        capability_tests = read(UPDATE_CAPABILITY_TESTS)
        self.assertIn('env!("CARGO_PKG_VERSION")', capability_tests)
        self.assertIn(
            'const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");',
            capability_tests,
        )

        product_version = workspace_package_version()
        for path in (
            APP_SETTINGS_SNAPSHOT,
            UI_LIBRARY,
            UI_WINDOW_COPY_TESTS,
            UPDATE_RUNTIME_TESTS,
            UPDATE_CAPABILITY_TESTS,
        ):
            with self.subTest(path=path):
                self.assertNotIn(
                    product_version,
                    read(path),
                    "product source and tests must derive the version from Cargo",
                )

    def test_macos_bundle_version_is_generated_at_package_time(self):
        with MACOS_INFO.open("rb") as source:
            info = plistlib.load(source)

        # The overlay must not carry a version; cargo-packager writes
        # CFBundleShortVersionString from the configured product version and a
        # generated CFBundleVersion build number.
        self.assertNotIn("CFBundleShortVersionString", info)
        self.assertNotIn("CFBundleVersion", info)

        packager = read(PACKAGER)
        self.assertIn('config.version = env!("CARGO_PKG_VERSION").to_owned();', packager)
        self.assertIn("macos.info_plist_path = Some(", packager)
        self.assertIn("version.workspace = true", read(PACKAGER_MANIFEST))

    def test_windows_metadata_is_generated_from_the_workspace_version(self):
        app_build_script = read(APP_BUILD_SCRIPT)
        for cargo_version_constant in (
            "CARGO_PKG_VERSION",
            "CARGO_PKG_VERSION_MAJOR",
            "CARGO_PKG_VERSION_MINOR",
            "CARGO_PKG_VERSION_PATCH",
        ):
            with self.subTest(constant=cargo_version_constant):
                self.assertIn(cargo_version_constant, app_build_script)

        resource = read(WINDOWS_RESOURCE)
        self.assertIn("VERSIONINFO", resource)
        self.assertIn("FILEVERSION VERSION_MAJOR,VERSION_MINOR,VERSION_PATCH,0", resource)
        self.assertIn("PRODUCTVERSION VERSION_MAJOR,VERSION_MINOR,VERSION_PATCH,0", resource)
        self.assertIn('VALUE "FileVersion", VERSION "\\0"', resource)
        self.assertIn('VALUE "ProductVersion", VERSION "\\0"', resource)

        # The installer product version comes from the same configured value; the
        # packaging pipeline never restates it.
        packager = read(PACKAGER)
        self.assertIn('config.version = env!("CARGO_PKG_VERSION").to_owned();', packager)

        product_version = workspace_package_version()
        for path in (APP_BUILD_SCRIPT, WINDOWS_RESOURCE, PACKAGER):
            with self.subTest(path=path):
                self.assertNotIn(
                    product_version,
                    read(path),
                    "packaging metadata must be derived from Cargo, not restated",
                )

    def test_build_documentation_uses_a_version_placeholder(self):
        documentation = read(PRODUCT_RUNTIME_DOC)
        self.assertIn("one source of truth", documentation)
        self.assertIn("Cargo.lock` records the resolved workspace versions", documentation)
        self.assertNotIn("-ProductVersion", documentation)
        self.assertNotIn(workspace_package_version(), documentation)


if __name__ == "__main__":
    unittest.main()
