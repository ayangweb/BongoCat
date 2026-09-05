import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "native" / "windows" / "installer" / "BongoCat.nsi"
PACKAGER = ROOT / "native" / "scripts" / "package-windows.ps1"


class WindowsInstallerContractTests(unittest.TestCase):
    def test_nsi_stays_current_user_and_fixed_root_only(self):
        source = INSTALLER.read_text(encoding="utf-8")

        self.assertIn("Unicode true", source)
        self.assertIn("RequestExecutionLevel user", source)
        self.assertIn('InstallDir "$LOCALAPPDATA\\Programs\\BongoCat"', source)
        self.assertNotIn("HKLM", source)
        self.assertNotIn("\\$" + "{", source)
        self.assertIn(
            'StrCmp $INSTDIR "$LOCALAPPDATA\\Programs\\BongoCat" 0 unexpected_install_directory',
            source,
        )
        self.assertIn('RMDir /r "$INSTDIR"', source)
        self.assertIn("File /r", source)
        self.assertIn("INPUT_DIRECTORY", source)
        self.assertIn('WriteUninstaller "$INSTDIR\\Uninstall.exe"', source)

    def test_packager_requires_signed_x64_provenance_payload(self):
        source = PACKAGER.read_text(encoding="utf-8")

        self.assertIn("$env:BONGOCAT_BUILD_ENV", source)
        self.assertIn("$expectedTarget = 'x86_64-pc-windows-msvc'", source)
        self.assertIn("$expectedNsisVersion = 'v3.11'", source)
        self.assertIn("$expectedNsisSetupMd5 = '700dc40097d4cd226b13212dda1d33ac'", source)
        self.assertIn("Get-AuthenticodeSignature", source)
        self.assertIn("[System.IO.FileAttributes]::ReparsePoint", source)
        self.assertIn("resources\\build-provenance.json", source)
        for model in ("standard", "keyboard", "gamepad"):
            self.assertIn(f"'{model}'", source)
        self.assertIn("InputDirectory must not contain Uninstall.exe", source)

    def test_packager_does_not_build_sign_or_fetch(self):
        source = PACKAGER.read_text(encoding="utf-8").lower()

        self.assertNotIn("cargo ", source)
        self.assertNotIn("invoke-webrequest", source)
        self.assertNotIn("start-process", source)
        self.assertNotIn("signtool", source)


if __name__ == "__main__":
    unittest.main()
