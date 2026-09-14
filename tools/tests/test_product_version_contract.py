"""Keep shipped BongoCat product metadata derived from one workspace value."""

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
APP_SETTINGS = ROOT / "crates" / "bongocat-app" / "src" / "settings.rs"
UI_LIBRARY = ROOT / "crates" / "bongocat-ui" / "src" / "lib.rs"
UI_WINDOW_TESTS = ROOT / "crates" / "bongocat-ui" / "src" / "window" / "tests.rs"
UPDATE_RUNTIME = ROOT / "crates" / "bongocat-update" / "src" / "runtime.rs"
UPDATE_CAPABILITY_TESTS = (
    ROOT / "crates" / "bongocat-update" / "tests" / "release_manifest_capability.rs"
)
PRODUCT_RUNTIME_DOC = ROOT / "docs" / "product-runtime.md"


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
        members = workspace_members()
        self.assertTrue(members, "the Native workspace must contain product crates")

        for member in members:
            with self.subTest(member=member):
                manifest = ROOT / member / "Cargo.toml"
                source = read(manifest)
                self.assertRegex(source, r"(?m)^version\.workspace\s*=\s*true\s*$")
                self.assertNotRegex(source, r"(?m)^version\s*=\s*\"")

    def test_lockfile_workspace_packages_match_the_product_version(self):
        expected_version = workspace_package_version()
        source = read(LOCKFILE)
        packages = dict(
            re.findall(
                r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"',
                source,
            )
        )
        workspace_packages = {
            name: version
            for name, version in packages.items()
            if name.startswith("bongocat-")
        }

        self.assertTrue(workspace_packages, "Cargo.lock must contain product packages")
        self.assertEqual(
            set(workspace_packages.values()),
            {expected_version},
            "every bongocat workspace package must inherit the product version",
        )

    def test_runtime_and_ui_use_the_compiled_product_version(self):
        self.assertIn(
            'pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");',
            read(APP_LIBRARY),
        )
        self.assertIn("product_version: PRODUCT_VERSION.to_owned()", read(APP_SETTINGS))
        self.assertIn(
            'product_version: env!("CARGO_PKG_VERSION").to_owned()',
            read(UI_LIBRARY),
        )
        self.assertIn('let product_version = env!("CARGO_PKG_VERSION");', read(UI_WINDOW_TESTS))
        self.assertIn('format!("Version {product_version} · Development")', read(UI_WINDOW_TESTS))
        self.assertIn(
            'format!("版本 {product_version} · 开发环境")',
            read(UI_WINDOW_TESTS),
        )

        update_runtime = read(UPDATE_RUNTIME)
        self.assertIn('env!("CARGO_PKG_VERSION")', update_runtime)

        capability_tests = read(UPDATE_CAPABILITY_TESTS)
        self.assertIn('env!("CARGO_PKG_VERSION")', capability_tests)
        self.assertIn(
            'const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");',
            capability_tests,
        )

        product_version = workspace_package_version()
        for path in (
            APP_SETTINGS,
            UI_LIBRARY,
            UI_WINDOW_TESTS,
            UPDATE_RUNTIME,
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
