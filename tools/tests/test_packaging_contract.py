"""Pin what the packaging pipeline guarantees and `cargo-packager` cannot vouch for.

The build and packaging entry point is `crates/bongocat-packaging`, which drives
`cargo-packager` (see `docs/adr/0033-build-packaging-and-release-toolchain.md`).
`cargo-packager` owns bundle layout, `Info.plist` generation and the NSIS
installer, so this file no longer asserts their internals. What it keeps is the
set of decisions only this repository can make:

* which targets ship and which artifacts each target publishes,
* that the Windows installer stays per-user,
* that the Windows installer is published under the product release name,
* that the macOS bundle overlay and the runtime resource lookup agree,
* that `just` stays a thin entry point and no self-built packaging script returns.

Those were the guarantees the deleted `scripts/` and `windows/installer/BongoCat.nsi`
used to enforce by construction.
"""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PACKAGER = ROOT / "crates" / "bongocat-packaging" / "src" / "main.rs"
PACKAGER_MANIFEST = ROOT / "crates" / "bongocat-packaging" / "Cargo.toml"
JUSTFILE = ROOT / "justfile"
MACOS_INFO = ROOT / "macos" / "Info.plist"
APP_MAIN = ROOT / "crates" / "bongocat-app" / "src" / "main.rs"
UPDATE_RELEASE = ROOT / "crates" / "bongocat-update" / "src" / "release.rs"
DEPENDENCY_POLICY = ROOT / "deny.toml"
WORKFLOW_DIRECTORY = ROOT / ".github" / "workflows"
RELEASE_WORKFLOW = WORKFLOW_DIRECTORY / "release.yml"


def read(path):
    return path.read_text(encoding="utf-8")


def shipped_targets():
    """The release targets declared by the packaging tool, in declaration order."""
    body = re.search(r"const ALL: \[Self; 3\] = \[(.*?)\];", read(PACKAGER), re.DOTALL)
    if body is None:
        raise AssertionError("the packaging tool must declare its release target list")
    return re.findall(r"Self::([A-Za-z0-9_]+)", body.group(1))


class PackagingTargetTests(unittest.TestCase):
    def test_exactly_three_release_targets_ship(self):
        variants = re.search(r"enum ReleaseTarget \{(.*?)\n\}", read(PACKAGER), re.DOTALL)
        self.assertIsNotNone(variants, "ReleaseTarget must exist")

        declared = re.findall(r"^    ([A-Za-z0-9_]+),$", variants.group(1), re.MULTILINE)
        self.assertEqual(len(declared), 3, f"expected three targets, found {declared}")
        self.assertEqual(len(shipped_targets()), 3)

        source = read(PACKAGER)
        for target in ("aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"):
            with self.subTest(target=target):
                self.assertIn(f'"{target}"', source)

        # Windows ARM64 is not shipped: Cubism R5 has no desktop ARM64 Core and
        # Windows runs the x64 build under emulation.
        self.assertNotIn("aarch64-pc-windows-msvc", source)
        self.assertNotIn("i686-pc-windows-msvc", source)

    def test_every_release_target_publishes_the_expected_artifacts(self):
        body = re.search(
            r"const fn release_formats\(self\).*?match self \{(.*?)\n        \}",
            read(PACKAGER),
            re.DOTALL,
        )
        self.assertIsNotNone(body, "release_formats must declare the artifact set")
        apple, windows = body.group(1).split("Self::WindowsX86_64")

        self.assertIn("PackageFormat::App", apple)
        self.assertIn("PackageFormat::Dmg", apple)
        self.assertIn("PackageFormat::Nsis", windows)
        self.assertNotIn("PackageFormat::Wix", body.group(1), "MSI is not a BongoCat artifact")

    def test_windows_installer_stays_per_user(self):
        source = read(PACKAGER)
        self.assertIn("NSISInstallerMode::CurrentUser", source)
        self.assertNotIn("NSISInstallerMode::PerMachine", source)
        self.assertNotIn("NSISInstallerMode::Both", source)

    def test_windows_installer_is_published_under_the_product_release_name(self):
        # cargo-packager names the NSIS installer after the main binary and
        # appends `-setup`, and exposes no option for it. The published name is a
        # product decision, so the packaging entry point renames the finished
        # installer and the release workflow asserts the exact name.
        source = read(PACKAGER)
        self.assertIn("fn installer_file_name(self)", source)
        self.assertIn("fn rename_windows_installer(", source)
        self.assertIn('"{PRODUCT_NAME}_{}_{}.exe"', source)
        self.assertIn("rename_windows_installer(target, &mut artifacts)?;", source)

        workflow = read(RELEASE_WORKFLOW)
        self.assertIn("BongoCat_${env:version}_x64.exe", workflow)
        # The installer is the Windows update payload, so it is uploaded together with
        # the signature and the manifest the updater reads.
        for uploaded in (
            "target/package/BongoCat_*.exe",
            "target/package/BongoCat_*.exe.sig",
            "target/package/windows-x86_64.json",
        ):
            with self.subTest(uploaded=uploaded):
                self.assertIn(uploaded, workflow)
        self.assertNotIn("-setup", workflow, "the published installer drops the packaging suffix")


class MacosBundleTests(unittest.TestCase):
    def test_bundle_identity_and_minimum_version_are_single_sourced(self):
        source = read(PACKAGER)
        self.assertIn('const BUNDLE_IDENTIFIER: &str = "com.ayangweb.bongo-cat";', source)
        self.assertIn('const MACOS_MINIMUM_SYSTEM_VERSION: &str = "12.0";', source)
        self.assertIn("macos.minimum_system_version = Some(MACOS_MINIMUM_SYSTEM_VERSION", source)
        self.assertIn("macos.info_plist_path = Some(", source)

        overlay = read(MACOS_INFO)
        # Keys cargo-packager generates must not also live in the overlay, otherwise
        # the bundle carries two sources for the same value and they can drift.
        for generated in (
            "CFBundleIdentifier",
            "CFBundleShortVersionString",
            "CFBundleVersion",
            "CFBundleExecutable",
            "CFBundleIconFile",
            "LSMinimumSystemVersion",
        ):
            with self.subTest(key=generated):
                self.assertNotIn(f"<key>{generated}</key>", overlay)

        self.assertIn("<key>LSMultipleInstancesProhibited</key>", overlay)
        self.assertIn("<key>NSPrincipalClass</key>", overlay)

    def test_bundled_resources_match_the_runtime_lookup(self):
        source = read(PACKAGER)
        app = read(APP_MAIN)

        # macOS resolves Contents/Resources/models; Windows resolves
        # <executable dir>/resources/models. The packaging tool must produce exactly
        # those two resource targets or the product silently ships without models.
        self.assertIn('Some(contents.join("Resources/models"))', app)
        self.assertIn('Some(executable.parent()?.join("resources/models"))', app)
        self.assertIn("let prefix = if target.is_apple()", source)
        self.assertIn('format!("{RESOURCE_DIRECTORY}/")', source)
        self.assertIn('PathBuf::from(format!("{prefix}{MODEL_DIRECTORY}"))', source)
        self.assertIn('PathBuf::from(format!("{prefix}{PROVENANCE_FILE}"))', source)

    def test_bundle_is_rejected_when_the_resources_are_missing(self):
        source = read(PACKAGER)
        self.assertIn('const PRESET_MODELS: [&str; 3] = ["standard", "keyboard", "gamepad"];', source)
        self.assertIn("fn verify_app_bundle(", source)

    def test_disk_image_wraps_the_packaged_bundle(self):
        source = read(PACKAGER)
        # The DMG must wrap the bundle cargo-packager produced rather than
        # reimplement bundle assembly. See ADR-0033 for why the DMG is not
        # delegated to cargo-packager.
        self.assertIn("fn packager_formats(requested: &[PackageFormat])", source)
        self.assertIn("fn build_disk_image(", source)
        self.assertIn('Command::new("hdiutil")', source)
        self.assertIn('symlink("/Applications"', source)


class BuildEntryPointTests(unittest.TestCase):
    def test_just_stays_a_thin_entry_point(self):
        source = read(JUSTFILE)
        self.assertIn("cargo run --locked -p bongocat-packaging", source)

        lowered = source.lower()
        for build_logic in (
            "powershell",
            "hdiutil",
            "info.plist",
            "plutil",
            "makensis",
            "os()",
            "scripts/",
            "cp -r",
        ):
            with self.subTest(build_logic=build_logic):
                self.assertNotIn(build_logic, lowered, "the Justfile must not carry build logic")

    def test_version_has_exactly_one_source(self):
        source = read(PACKAGER)
        self.assertIn('config.version = env!("CARGO_PKG_VERSION").to_owned();', source)
        self.assertIn('"--print-version"', source)
        self.assertIn("version.workspace = true", read(PACKAGER_MANIFEST))
        self.assertIn("just version", read(RELEASE_WORKFLOW))

    def test_the_compiled_environment_reaches_the_child_build(self):
        source = read(PACKAGER)
        self.assertIn('.env("BONGOCAT_BUILD_ENV", environment)', source)
        self.assertIn('const BUILD_ENVIRONMENTS: [&str; 2] = ["development", "production"];', source)
        self.assertIn('Command::new(&cargo)', source)
        self.assertIn('"-p",', source)

    def test_no_self_built_packaging_script_remains(self):
        self.assertFalse((ROOT / "scripts").exists(), "scripts/ must be deleted")
        self.assertFalse(
            (ROOT / "windows" / "installer" / "BongoCat.nsi").exists(),
            "the hand-written NSIS script must be deleted",
        )
        for workflow in sorted(WORKFLOW_DIRECTORY.glob("*.y*ml")):
            with self.subTest(workflow=workflow.name):
                source = read(workflow)
                self.assertNotIn("scripts/", source)
                self.assertNotIn("BongoCat.nsi", source)


class ReleaseMatrixTests(unittest.TestCase):
    def triples(self):
        mapping = {
            "MacosAarch64": "aarch64-apple-darwin",
            "MacosX86_64": "x86_64-apple-darwin",
            "WindowsX86_64": "x86_64-pc-windows-msvc",
        }
        return [mapping[variant] for variant in shipped_targets()]

    def update_targets(self):
        block = re.search(
            r"impl UpdateTargetTriple \{(.*?)\n\}", read(UPDATE_RELEASE), re.DOTALL
        )
        self.assertIsNotNone(block, "UpdateTargetTriple must keep an inherent impl")
        return sorted(set(re.findall(r'Self::\w+ => "([^"]+)"', block.group(1))))

    def test_release_workflow_builds_every_shipped_target(self):
        source = read(RELEASE_WORKFLOW)
        for target in self.triples():
            with self.subTest(target=target):
                self.assertIn(target, source)

    def test_release_workflow_runs_the_same_entry_point_as_developers(self):
        source = read(RELEASE_WORKFLOW)
        self.assertIn("just build", source)
        self.assertNotIn("cargo packager", source)
        self.assertNotIn("cargo install cargo-packager", source)

    def test_update_runtime_and_dependency_policy_match_the_shipped_targets(self):
        expected = sorted(self.triples())

        graph = read(DEPENDENCY_POLICY).split("[licenses]")[0]
        self.assertEqual(sorted(re.findall(r'"([a-z0-9_]+-[a-z0-9_-]+)"', graph)), expected)
        self.assertEqual(self.update_targets(), expected)

    def test_packaging_targets_match_the_update_runtime(self):
        # Both lists must stay in step; the packaging tool deliberately does not
        # depend on bongocat-update to keep its dependency tree small.
        self.assertEqual(sorted(self.triples()), self.update_targets())


if __name__ == "__main__":
    unittest.main()
