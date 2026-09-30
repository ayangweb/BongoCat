"""Keep the Swift runtime rpath on every package whose binaries link it.

`bongocat-platform` links the Swift static library that backs the macOS permission
flow (ADR-0078). A library dependency's `rustc-link-arg` does not reach the binary
that finally links, so each package whose binaries pull that library in has to add
the flag itself. A package that forgets it still compiles and still passes
`cargo check`; it only fails when one of its binaries runs, with

    dyld: Library not loaded: @rpath/libswift_Concurrency.dylib

so nothing but this test catches the omission before a user does. Adding a new
crate that depends on `bongocat-platform` is exactly the case it exists for.
"""

import re
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CRATES = ROOT / "crates"

#: The flag a build script has to emit, in either cargo instruction spelling.
RPATH_FLAG = "-Wl,-rpath,/usr/lib/swift"

#: The package that links the Swift library.
SWIFT_LINKER = "bongocat-platform"

DEPENDENCY = re.compile(r"^\s*(bongocat-[a-z0-9-]+)\s*(?:\.workspace\s*)?=", re.MULTILINE)


def package_dependencies(crate: Path) -> set:
    """Workspace packages `crate` depends on, from its manifest."""
    manifest = crate / "Cargo.toml"
    if not manifest.is_file():
        return set()
    source = manifest.read_text(encoding="utf-8")
    # The package's own name is declared as `name = "..."`, which the dependency
    # pattern does not match, so the set only ever holds real dependencies.
    return set(DEPENDENCY.findall(source))


def packages_linking_swift(root=CRATES) -> set:
    """Every workspace package that ends up linking the Swift library."""
    dependencies = {crate.name: package_dependencies(crate) for crate in root.iterdir() if crate.is_dir()}
    linking = {name for name, deps in dependencies.items() if SWIFT_LINKER in deps}
    linking.add(SWIFT_LINKER)
    # Transitive closure: a package that depends on a linker also links the library.
    changed = True
    while changed:
        changed = False
        for name, deps in dependencies.items():
            if name not in linking and deps & linking:
                linking.add(name)
                changed = True
    return linking


def missing_rpath(root=CRATES) -> list:
    """Return `package: reason` for every linker whose binaries would fail to load."""
    reported = []
    for name in sorted(packages_linking_swift(root)):
        build_script = root / name / "build.rs"
        if not build_script.is_file():
            reported.append(f"{name}: no build.rs, so its binaries carry no LC_RPATH")
            continue
        source = build_script.read_text(encoding="utf-8")
        if RPATH_FLAG not in source:
            reported.append(f"{name}: build.rs does not emit {RPATH_FLAG}")
        elif "CARGO_CFG_TARGET_OS" not in source:
            reported.append(f"{name}: build.rs emits the rpath without the macOS guard")
    return reported


class SwiftRuntimeRpathContractTests(unittest.TestCase):
    def test_every_package_linking_swift_carries_the_rpath(self):
        reported = missing_rpath()
        self.assertEqual(reported, [], "\n".join(reported))

    def test_the_scan_finds_the_packages_it_is_supposed_to(self):
        # Reverse self-check: the contract is only meaningful while the scan sees
        # the packages that really link the library. Without this, a broken
        # dependency pattern would turn the assertion above into an empty one.
        linking = packages_linking_swift()
        self.assertIn(SWIFT_LINKER, linking)
        self.assertIn("bongocat-app", linking)
        self.assertIn("bongocat-overlay", linking)
        self.assertIn("bongocat-ui", linking)
        # A package with no path to the linker must not be dragged in.
        self.assertNotIn("bongocat-log", linking)

    def test_a_build_script_without_the_flag_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / SWIFT_LINKER).mkdir()
            (root / SWIFT_LINKER / "Cargo.toml").write_text(
                '[package]\nname = "bongocat-platform"\n', encoding="utf-8"
            )
            (root / SWIFT_LINKER / "build.rs").write_text(
                'fn main() {\n    println!("cargo:rerun-if-changed=src");\n}\n',
                encoding="utf-8",
            )
            reported = missing_rpath(root)
            self.assertEqual(len(reported), 1, reported)
            self.assertIn(RPATH_FLAG, reported[0])


if __name__ == "__main__":
    unittest.main()
