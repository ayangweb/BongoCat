"""Pin what the packaging pipeline guarantees and `cargo-packager` cannot vouch for.

The build and packaging entry point is `crates/bongocat-packaging`, which drives
`cargo-packager` (see `docs/adr/0033-build-packaging-and-release-toolchain.md`).
`cargo-packager` owns bundle layout, `Info.plist` generation and the NSIS
installer, so this file no longer asserts their internals. What it keeps is the
set of decisions only this repository can make:

* which targets ship and which artifacts each target publishes,
* that the Windows installer stays per-user,
* that the Windows installer offers exactly the application languages,
* that the Windows installer is published under the release name,
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
FINDER_STORE = ROOT / "crates" / "bongocat-packaging" / "src" / "finder_store.rs"
PACKAGER_MANIFEST = ROOT / "crates" / "bongocat-packaging" / "Cargo.toml"
I18N_LOCALES = ROOT / "crates" / "bongocat-i18n" / "locales"
VIETNAMESE_INSTALLER_STRINGS = (
    ROOT / "crates" / "bongocat-packaging" / "installer" / "Vietnamese.nsh"
)
JUSTFILE = ROOT / "justfile"
MACOS_INFO = ROOT / "macos" / "Info.plist"
APP_PRESET_ROOT = ROOT / "crates" / "bongocat-app" / "src" / "preset_root.rs"
WINDOWS_RESOURCE = ROOT / "crates" / "bongocat-app" / "windows" / "bongocat-app.rc"
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

    def test_windows_installer_offers_exactly_the_application_languages(self):
        # Each application catalog maps to the NSIS language of the same variety. A new
        # catalog without an installer language (or the reverse) fails here.
        nsis_languages = {
            "en-US": "English",
            "zh-CN": "SimpChinese",
            "zh-TW": "TradChinese",
            "ar-SA": "Arabic",
            "vi-VN": "Vietnamese",
            "pt-BR": "PortugueseBR",
            "ko-KR": "Korean",
        }
        catalogs = {path.stem for path in I18N_LOCALES.glob("*.json")}
        self.assertEqual(catalogs, set(nsis_languages))

        source = read(PACKAGER)
        declared = re.search(r"const INSTALLER_LANGUAGES: \[&str; \d+\] = \[(.*?)\];", source, re.DOTALL)
        self.assertIsNotNone(declared, "the installer language list must be declared")
        languages = re.findall(r"\"([A-Za-z]+)\"", declared.group(1))
        self.assertEqual(sorted(languages), sorted(nsis_languages.values()))
        self.assertEqual(languages[0], "English", "English is the fallback language")

        # cargo-packager embeds no strings for Vietnamese, so the repository carries the
        # messages its installer template looks up.
        raw = VIETNAMESE_INSTALLER_STRINGS.read_bytes()
        self.assertTrue(raw.startswith(b"\xef\xbb\xbf"), "NSIS needs the UTF-8 BOM")
        names = re.findall(
            r"^LangString (\w+) \$\{LANG_VIETNAMESE\} ",
            raw.decode("utf-8-sig"),
            re.MULTILINE,
        )
        self.assertEqual(
            sorted(names),
            sorted(
                [
                    "addOrReinstall", "alreadyInstalled", "alreadyInstalledLong",
                    "appRunning", "appRunningOkKill", "chooseMaintenanceOption",
                    "choowHowToInstall", "createDesktop", "deleteAppData",
                    "dontUninstall", "dontUninstallDowngrade", "failedToKillApp",
                    "newerVersionInstalled", "older", "olderOrUnknownVersionInstalled",
                    "silentDowngrades", "unableToUninstall", "uninstallApp",
                    "uninstallBeforeInstalling", "unknown",
                ]
            ),
        )

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

    def test_release_jobs_and_the_release_notes_call_a_machine_the_same_thing(self):
        # A reader sees these in two places: the checks list on the tag and the
        # download block on the release page. They used to disagree on every
        # macOS job, and the two rows of that block used two words for the same
        # chip, so the same machine was named four ways in one release.
        workflow = read(RELEASE_WORKFLOW)
        packager = read(PACKAGER)

        self.assertIn("name: Build Windows x64", workflow)
        self.assertIn("name: Build macOS ${{ matrix.label }}", workflow)
        self.assertNotIn("Build macOS ${{ matrix.triple }}", workflow)
        for label in ("Apple Silicon", "Intel"):
            with self.subTest(label=label):
                self.assertIn(f"- label: {label}", workflow)
        # The exact strings the two locales assign, rather than a search for a
        # misspelling anywhere in the file: the reasoning next to the constants
        # names the rejected spellings on purpose.
        for assigned in (
            'windows_label: "Windows 10+"',
            'windows_architecture_label: "x64"',
            'apple_silicon_label: "Apple Silicon"',
            'intel_label: "Intel"',
        ):
            with self.subTest(assigned=assigned):
                self.assertEqual(packager.count(assigned), 2, f"both locales must assign {assigned}")
        for rejected in (
            'windows_label: "Windows 10 1903+"',
            'apple_silicon_label: "Apple silicon"',
            'apple_silicon_label: "Apple 芯片"',
            'intel_label: "Intel 芯片"',
        ):
            with self.subTest(rejected=rejected):
                self.assertNotIn(rejected, packager)

    def test_every_release_job_is_named_by_the_action_it_performs(self):
        # The checks list stacks this workflow's jobs next to the other workflows'
        # and the reader scans them as one column. Three jobs reading `Windows x64`,
        # `macOS Apple Silicon`, `macOS Intel` next to `Publish the GitHub release`
        # reads as two different naming schemes, and the publish job is the one that
        # is out of step. So every job is verb-first, and the object is
        # `<platform> <chip>`.
        #
        # The verb names the action, never the artifact: this workflow's Windows
        # target publishes an installer and its macOS targets publish a bundle and a
        # disk image, so naming either in the job name would make the three read as
        # three different products. A reader who needs the file follows the download
        # link in the release notes.
        workflow = read(RELEASE_WORKFLOW)
        names = re.findall(r"^  \S+:\n    name: (.+)$", workflow, re.MULTILINE)
        self.assertEqual(len(names), 3, f"expected three jobs, got {names}")
        for name in names:
            with self.subTest(job=name):
                self.assertRegex(
                    name,
                    r"^(Build|Publish) \S+",
                    "every job name is a verb followed by what it acts on",
                )
        for absent in ("App", "app.tar.gz", "apple-darwin"):
            self.assertNotIn(
                absent,
                "".join(names),
                "a job name must not carry an artifact name or a target triple",
            )

    def test_a_step_named_require_actually_fails_the_build(self):
        # A step called "Require X" that only warns is worse than no name at all:
        # a maintainer scanning a green log reads it as proof X was enforced. The
        # update-signing gate really does exit 1; the Authenticode step cannot,
        # because CI has no certificate configured, so it is named for what it does.
        workflow = read(RELEASE_WORKFLOW)

        enforcing = re.findall(r"- name: (Require [^\n]+)\n(.*?)(?=\n      - )", workflow, re.DOTALL)
        self.assertTrue(enforcing, "the release must keep a real signing gate")
        for name, body in enforcing:
            with self.subTest(step=name):
                self.assertTrue(
                    "exit 1" in body,
                    f"{name} does not fail the build, so it must not be named Require",
                )

        reporting = re.search(
            r"- name: (Report Authenticode[^\n]+)\n(.*?)(?=\n      - )", workflow, re.DOTALL
        )
        self.assertIsNotNone(reporting, "the Authenticode step must say it reports")
        self.assertIn("::warning::", reporting.group(2))
        self.assertNotIn("exit 1", reporting.group(2))
        self.assertNotIn("Authenticode signatures for a stable release\"", workflow)

    def test_both_build_jobs_name_the_build_step_the_same_way(self):
        # The two jobs run the same operation on different machines, and the
        # product vocabulary differs downstream (installer vs bundle and disk
        # image) because the artifacts really do. Naming that difference here too
        # made one step read as two unrelated steps; the Verify steps below already
        # say what each target produced.
        workflow = read(RELEASE_WORKFLOW)
        builds = re.findall(r"- name: (Build the Production product[^\n]+)", workflow)
        self.assertEqual(len(builds), 2, f"expected one build step per job, got {builds}")
        self.assertEqual(len(set(builds)), 1, f"the build steps must read identically: {builds}")

    def test_every_release_artifact_is_keyed_by_the_architecture_token(self):
        # The job a reader sees on the tag is named by chip, but the uploaded
        # artifact is the key `download-artifact` matches on, and the macOS leg
        # still interpolated the raw triple there — so one release produced job
        # names reading `macOS Apple Silicon` next to an artifact called
        # `release-macos-aarch64-apple-darwin`.
        workflow = read(RELEASE_WORKFLOW)
        for key in (
            "name: release-windows-x64",
            "name: release-macos-${{ matrix.arch }}",
        ):
            with self.subTest(key=key):
                self.assertIn(key, workflow)

        # Every `release-*` key is `<os>-<arch>`, never a triple. Match to the end of
        # the line so a `${{ matrix.* }}` interpolation is captured whole rather than
        # truncated at its first space, which would make the check pass vacuously.
        keys = re.findall(r"^\s*name: (release-\S.*?)\s*$", workflow, re.MULTILINE)
        self.assertEqual(len(keys), 2, f"expected two release artifact keys, got {keys}")
        for key in keys:
            with self.subTest(key=key):
                self.assertNotIn("apple-darwin", key)
                self.assertNotIn("pc-windows-msvc", key)
        self.assertNotIn("release-macos-${{ matrix.triple }}", workflow)
        # The Windows leg has no matrix, so its `x64` is written out. It has to be
        # the same token `ReleaseTarget::architecture` returns for that target.
        self.assertIn(
            'Self::WindowsX86_64 => "x64",',
            read(PACKAGER),
            "the Windows artifact key must be the target's own architecture token",
        )

    def test_the_published_changelogs_call_a_chip_by_one_name(self):
        # The changelog entry is fed verbatim into the release body and into the
        # in-app update window, so a chip spelled one way there and another in the
        # generated download block puts two names for one machine in one document.
        # The 2.0.0 entry said "Intel/Apple silicon" in English and
        # "Intel/Apple 芯片" in Chinese, next to an appendix that says
        # "Apple Silicon".
        for name in ("CHANGELOG.md", "CHANGELOG.zh-CN.md"):
            with self.subTest(changelog=name):
                changelog = read(ROOT / name)
                for rejected in ("Apple silicon", "Apple 芯片", "Intel 芯片"):
                    self.assertNotIn(rejected, changelog)
                self.assertIn("Apple Silicon", changelog)

    def test_every_macos_artifact_is_named_after_the_same_architecture_token(self):
        # The disk image, the bundle archive and the manifest key of one machine used
        # to spell its architecture three ways: `arm64.dmg`, `aarch64.app.tar.gz` and
        # `macos-aarch64`. The release notes link the first and the updater installs
        # the second, so a drift between them is a download link to an asset the
        # release never uploads, and the build stays green.
        packager = read(PACKAGER)
        token = re.search(r"const fn architecture\(self\).*?\n        \}", packager, re.DOTALL)
        self.assertIsNotNone(token, "ReleaseTarget::architecture must declare the token")
        self.assertIn('Self::MacosAarch64 => "aarch64"', token.group(0))
        self.assertNotIn("arm64", token.group(0), "the published token is aarch64")

        # Both published macOS names and the fragment name are built from that one
        # token, so the workflow can assert them from a single matrix value.
        for built in (
            '"{PRODUCT_NAME}-{}-{}.dmg"',
            '"{PRODUCT_NAME}-{}-{}.app.tar.gz"',
            'format!("{}{UPDATE_FRAGMENT_SUFFIX}", target.manifest_platform())',
        ):
            with self.subTest(built=built):
                self.assertIn(built, packager)

        workflow = read(RELEASE_WORKFLOW)
        for built in (
            "BongoCat-$version-${{ matrix.arch }}.dmg",
            "BongoCat-$version-${{ matrix.arch }}.app.tar.gz",
        ):
            with self.subTest(built=built):
                self.assertIn(built, workflow)
        self.assertNotIn("matrix.payload_arch", workflow)


class WindowsProductIconTests(unittest.TestCase):
    """The shipped executable has to carry the product icon where GPUI reads it.

    GPUI loads the application icon with
    `LoadImageW(module, MAKEINTRESOURCE(1), IMAGE_ICON, ...)` while it registers the
    window class, and Windows paints a window whose class icon is missing with its
    generic default icon. The resource id is therefore a product guarantee, not a
    free choice inside the resource script.
    """

    def test_product_icon_is_embedded_at_the_id_gpui_loads(self):
        resource = read(WINDOWS_RESOURCE)
        declarations = re.findall(
            r'^\s*(\S+)\s+ICON\s+"?([^"\s]+)"?\s*$', resource, re.MULTILINE
        )
        self.assertEqual(
            len(declarations),
            1,
            "the resource script must declare exactly one product icon",
        )

        name, path = declarations[0]
        self.assertEqual(name, "1", "GPUI only looks the application icon up at id 1")
        self.assertTrue(
            path.endswith("icons/logo-windows.ico"),
            f"the embedded icon must be the shipped product icon, found {path}",
        )


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
        app = read(APP_PRESET_ROOT)

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

    def test_disk_image_declares_the_window_the_installer_opens_with(self):
        # The width and the origin are Tauri v2's documented `bundle.macOS.dmg`
        # defaults; the height and the icon and label sizes are sized for the three
        # items this product puts in the window. See ADR-0075 and ADR-0076.
        source = read(PACKAGER)
        layout = read(FINDER_STORE)
        for value in (
            "width: 660",
            "height: 420",
            "origin: (10, 60)",
            "icon_size: 96",
            "text_size: 13",
        ):
            with self.subTest(value=value):
                self.assertIn(value, layout)

        # The three items, and the inverted triangle they are arranged in.
        for value in (
            "finder_store::Item::new(&bundle_name, 175, 110)",
            "finder_store::Item::new(APPLICATIONS_LINK, 485, 110)",
            "finder_store::Item::new(REPAIR_COMMAND, 330, 255)",
        ):
            with self.subTest(value=value):
                self.assertIn(value, source)

        self.assertIn("mod finder_store;", source)
        self.assertIn("finder_store::window(", source)
        # The layout is written into the volume, not arranged by Finder: driving
        # Finder needs a graphical session and an Automation consent prompt,
        # which an unattended release job does not have. The one AppleScript call
        # in this crate is the shipped repair script closing its own window, so
        # the invariant is that the build runs none.
        self.assertNotIn('Command::new("osascript")', source)
        self.assertNotIn("osascript", layout)

    def test_disk_image_carries_a_repair_command_for_the_installed_copy(self):
        # A downloaded, unnotarized bundle carries the quarantine attribute
        # Gatekeeper puts on downloads, and removing it needs a password typed
        # into a terminal, so the installer ships that as a command script
        # rather than leaving the reader with a manual command to look up.
        source = read(PACKAGER)
        self.assertIn('const REPAIR_COMMAND: &str = "Fix Damaged App";', source)
        self.assertIn("fn repair_command(app_name: &str) -> String", source)
        # Both forms, because macOS 15 rejects the recursive one.
        self.assertIn("sudo xattr -r -d com.apple.quarantine", source)
        self.assertIn("sudo xattr -d com.apple.quarantine", source)
        # Nothing here may weaken the machine: the reference tools this one is
        # modelled on also switch Gatekeeper off system-wide.
        self.assertNotIn("spctl", source)
        # It only repairs; re-signing is reported as a manual command instead.
        self.assertIn("sudo codesign --force --deep --sign -", source)

    def test_the_repair_command_says_what_to_do_when_there_is_nothing_to_repair(self):
        # A reader who has not installed the app yet should be told to install
        # it, not told there is nothing wrong: the two states are told apart, and
        # neither asks for a password it does not need.
        source = read(PACKAGER)
        for value in (
            'if [ ! -d "$app_dir" ]; then',
            "Drag {app_name} from this disk image into Applications first, then run",
            "Either that copy is already repaired, or it is not the copy from this",
            "onto Applications, replacing the copy that is there, then run this again",
        ):
            with self.subTest(value=value):
                self.assertIn(value, source)
        # The password is only asked for on the branch that needs it.
        self.assertLess(
            source.index("if ! xattr -p com.apple.quarantine"),
            source.index("Enter your Mac login password when asked"),
        )

    def test_the_repair_command_closes_the_window_it_ran_in(self):
        # Terminal leaves the window open when the command finishes, so the
        # prompt's promise is kept by the script. The close waits a second
        # because Terminal asks before terminating a running process, and it
        # matches the window by tty and title so a shell in use is not closed.
        source = read(PACKAGER)
        for value in (
            '"${{TERM_PROGRAM:-}}" = "Apple_Terminal"',
            "Press Enter to close this window...",
            "sleep 1",
            "first window whose tty is",
            "and name contains",
        ):
            with self.subTest(value=value):
                self.assertIn(value, source)
        # `tty` has to be read in the foreground: bash hands a background job
        # /dev/null as its input, and `tty` then answers "not a tty" instead of
        # naming the window. That is the version that silently never closes
        # anything, and it is invisible to a stubbed run.
        start = source.index("fn repair_command(app_name: &str) -> String {")
        command = source[start : source.index("\n}\n", start)]
        self.assertIn("this_tty=$(tty)", command)
        self.assertLess(
            command.index("this_tty=$(tty)"),
            command.index("\n    ) &\n"),
            "the tty has to be read before the close is put in the background",
        )

    def test_disk_image_gives_the_volume_the_application_icon(self):
        # Finder reads a volume's own icon from `.VolumeIcon.icns` plus a file
        # attribute on the mounted volume, so the image is built writable and
        # converted afterwards. See ADR-0075.
        source = read(PACKAGER)
        self.assertIn('const VOLUME_ICON_FILE: &str = ".VolumeIcon.icns";', source)
        self.assertIn("const MACOS_ICON: &str = \"icons/logo-macos.icns\";", source)
        self.assertIn('args(["-c", "icnC"])', source)
        self.assertIn('args(["-a", "C"])', source)
        self.assertIn('args(["-ov", "-format", "UDRW"])', source)
        self.assertIn('args(["-format", "ULMO", "-o"])', source)
        # A failed build must not leave the volume mounted, and a mount inside
        # the build tree leaks a filesystem-events journal into the artifact.
        self.assertIn("struct MountedImage", source)
        self.assertIn("impl Drop for MountedImage", source)
        self.assertIn("tempfile::TempDir::new()", source)


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
        self.assertIn('const PRODUCTION_FEATURE: &str = "production";', source)
        self.assertIn("fn environment_features(environment: &str) -> &'static str {", source)
        self.assertIn("command.args([\"--features\", features]);", source)
        self.assertIn('const BUILD_ENVIRONMENTS: [&str; 2] = ["development", "production"];', source)
        self.assertIn(".arg(features)", source)
        self.assertIn('Command::new(&cargo)', source)
        self.assertIn('"-p",', source)

    def test_app_uses_a_single_production_feature(self):
        manifest = read(ROOT / "crates" / "bongocat-app" / "Cargo.toml")
        self.assertIn("production = []", manifest)
        self.assertIn("storage-test-injection = []", manifest)
        self.assertNotIn("BONGOCAT_BUILD_ENV", read(PACKAGER))

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
