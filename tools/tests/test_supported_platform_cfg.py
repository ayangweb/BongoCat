"""Keep cfg predicates on the supported Windows/macOS platform set.

Linux is not a shipped target. A temporary compatibility union would make code
compile for a platform the contract test or release matrix is expected to catch.
"""

import re
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CRATES = ROOT / "crates"
CFG_ATTRIBUTE = re.compile(r"#\[cfg\s*\((.*?)\)\]", re.DOTALL)
ANY_EXPRESSION = re.compile(r"\bany\s*\((.*?)\)", re.DOTALL)
MACOS = 'target_os = "macos"'
WINDOWS = 'target_os = "windows"'
LINUX = 'target_os = "linux"'


def scan_rust_file(path):
    """Return ``(line_number, line, reason)`` for every forbidden cfg line."""
    source = path.read_text(encoding="utf-8")
    line_start = 1
    for match in CFG_ATTRIBUTE.finditer(source):
        expression = match.group(1)
        line_number = line_start + source[: match.start()].count("\n")
        line = source.splitlines()[line_number - 1]
        reasons = []
        if LINUX in expression:
            reasons.append("linux target_os")
        for any_expression in ANY_EXPRESSION.finditer(expression):
            if MACOS in any_expression.group(1) and WINDOWS in any_expression.group(1):
                reasons.append("macOS+Windows any() union")
                break
        if reasons:
            yield line_number, line, "; ".join(reasons)
        line_start += source[match.start():match.end()].count("\n")


def offenders(root=CRATES):
    lines = []
    for path in sorted(root.rglob("*.rs")):
        for line_number, line, reason in scan_rust_file(path):
            lines.append(f"{path}:{line_number}: {reason}: {line}")
    return lines


class SupportedPlatformCfgDetectorTests(unittest.TestCase):
    def test_forbidden_unions_and_linux_are_reported(self):
        cases = {
            "macos_windows.rs": '#[cfg(any(target_os = "macos", target_os = "windows"))]\n',
            "windows_macos.rs": '#[cfg(any(target_os = "windows", target_os = "macos"))]\n',
            "linux.rs": '#[cfg(target_os = "linux")]\n',
            "multiline.rs": '#[cfg(any(\n    target_os = "macos",\n    target_os = "windows",\n))]\n',
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, source in cases.items():
                (root / name).write_text(source, encoding="utf-8")

            reported = offenders(root)

        self.assertEqual(len(reported), len(cases))
        for name in cases:
            self.assertTrue(any(f"/{name}:1:" in line for line in reported), reported)

    def test_supported_cfgs_pass(self):
        source = "\n".join(
            (
                '#[cfg(target_os = "macos")]',
                '#[cfg(any(target_os = "windows", test))]',
                "#[cfg(all(target_os = \"macos\", target_arch = \"aarch64\"))]",
                "#[cfg(any(target_os = \"macos\", test))]",
            )
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "supported.rs"
            path.write_text(source, encoding="utf-8")
            self.assertEqual(offenders(Path(directory)), [])


class SupportedPlatformCfgContractTests(unittest.TestCase):
    def test_crates_contain_no_unsupported_platform_cfg(self):
        reported = offenders()
        self.assertEqual(reported, [], "\n".join(reported))


if __name__ == "__main__":
    unittest.main()
