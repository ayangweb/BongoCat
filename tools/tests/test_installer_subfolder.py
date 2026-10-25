"""Constrain the upstream-template extension and compile its native MUI callbacks."""

import hashlib
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TEMPLATE = ROOT / "crates/bongocat-packaging/installer/installer.nsi"
UPSTREAM_SHA256 = "3ff5fde09ae24dfc031ab91f939b290f394e2c6d3549f2c43c37414716a47b7a"
BLOCK = re.compile(
    r"; BongoCat extension begin\n(.*?); BongoCat extension end\n", re.DOTALL
)


class InstallerSubfolderTests(unittest.TestCase):
    def setUp(self):
        self.source = TEMPLATE.read_text(encoding="utf-8-sig")
        self.blocks = BLOCK.findall(self.source)

    def test_only_the_three_reviewed_extensions_differ_from_upstream(self):
        self.assertEqual(len(self.blocks), 3)
        upstream = BLOCK.sub("", self.source)
        self.assertEqual(hashlib.sha256(upstream.encode()).hexdigest(), UPSTREAM_SHA256)
        manifest = (ROOT / "Cargo.toml").read_text()
        self.assertIn('cargo-packager = { version = "=0.11.8"', manifest)
        self.assertTrue(TEMPLATE.read_bytes().startswith(b"\xef\xbb\xbf"))

    def test_all_installer_languages_have_the_checkbox_label(self):
        languages = re.findall(
            r"LangString bongoCatCreateSubfolder \$\{LANG_(\w+)\}", self.source
        )
        self.assertCountEqual(languages, [
            "ENGLISH", "SIMPCHINESE", "TRADCHINESE", "ARABIC", "VIETNAMESE",
            "PORTUGUESEBR", "KOREAN", "FRENCH",
        ])

    def test_path_mutation_is_scoped_to_the_visible_directory_page(self):
        callbacks, functions, init = self.blocks
        self.assertIn("MUI_PAGE_CUSTOMFUNCTION_SHOW BongoCatDirectoryShow", callbacks)
        self.assertIn("MUI_PAGE_CUSTOMFUNCTION_LEAVE BongoCatDirectoryLeave", callbacks)
        self.assertIn("StrCpy $BongoCatCreateSubfolder ${BST_CHECKED}", init)
        self.assertIn('ReadRegStr $0 SHCTX "${MANUPRODUCTKEY}" ""', init)
        self.assertIn('${OrIf} $INSTDIR != ""', init)
        self.assertIn("StrCpy $BongoCatCreateSubfolder ${BST_UNCHECKED}", init)
        self.assertNotIn("StrCpy $INSTDIR", init)
        self.assertIn("$INSTDIR == $BongoCatGeneratedDirectory", functions)
        self.assertIn("StrCpy $INSTDIR $BongoCatParentDirectory", functions)
        self.assertIn('StrCpy $INSTDIR "$INSTDIR\\${PRODUCTNAME}"', functions)
        self.assertIn("user32::MapDialogRect", functions)
        self.assertIn("${WM_SETFONT}", functions)
        self.assertIn("0x00400000", functions)

    def test_checkbox_and_space_labels_fit_the_upstream_directory_page(self):
        functions = self.blocks[1]
        positions = re.findall(r"!insertmacro BongoCatDialogRect (\d+) (\d+)", functions)
        intervals = sorted((int(y), int(y) + int(height)) for y, height in positions)
        self.assertEqual(intervals, [(106, 118), (122, 130), (132, 140)])
        self.assertGreater(intervals[0][0], 105)  # End of the upstream destination group.
        self.assertLessEqual(intervals[-1][1], 140)
        for previous, following in zip(intervals, intervals[1:]):
            self.assertLessEqual(previous[1], following[0])

    @unittest.skipUnless(shutil.which("makensis"), "NSIS compiler is not installed")
    def test_callbacks_compile_with_real_mui_and_every_language(self):
        callbacks, functions, init = self.blocks
        # The unchanged upstream template is hash-checked separately. This fixture
        # compiles every added instruction without requiring Windows product binaries
        # or cargo-packager's third-party installer plugins.
        languages = ["English", "SimpChinese", "TradChinese", "Arabic", "Vietnamese",
                     "PortugueseBR", "Korean", "French"]
        with tempfile.TemporaryDirectory(prefix="bongocat-nsis-") as directory:
            path = Path(directory)
            script = '\n'.join([
                'Unicode true', '!include MUI2.nsh', '!include FileFunc.nsh',
                '!define PRODUCTNAME "BongoCat"', '!define MANUPRODUCTKEY "Software\\BongoCat"',
                'Name "BongoCat compile fixture"', f'OutFile "{path / "fixture.exe"}"',
                'RequestExecutionLevel user', 'Var PassiveMode',
                'Function SkipIfPassive', 'FunctionEnd',
                '!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive', callbacks,
                '!insertmacro MUI_PAGE_DIRECTORY', '!insertmacro MUI_PAGE_INSTFILES',
                *[f'!insertmacro MUI_LANGUAGE "{language}"' for language in languages],
                functions, 'Function .onInit', init, 'FunctionEnd',
                'Section', 'SectionEnd',
            ])
            (path / "fixture.nsi").write_text(script, encoding="utf-8-sig")
            result = subprocess.run(
                [shutil.which("makensis"), "-V3", str(path / "fixture.nsi")],
                capture_output=True, text=True, timeout=60,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue((path / "fixture.exe").is_file())


if __name__ == "__main__":
    unittest.main()
