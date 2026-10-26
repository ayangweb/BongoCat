"""Contract for single-repository plugin development and publication.

The plugin SDK, official plugins, packaging, signing and plugin Release all
live in BongoCat.
"""

import base64
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKSPACE = ROOT / "Cargo.toml"
RELEASE_IDENTITY = (
    ROOT / "crates" / "bongocat-update" / "src" / "runtime" / "release_identity.rs"
)
RUNTIME_MOD = ROOT / "crates" / "bongocat-update" / "src" / "runtime" / "mod.rs"
APP_SOURCE = ROOT / "crates" / "bongocat-app" / "src" / "dev_plugins.rs"
DEV_PLUGINS = ROOT / "tools" / "dev_plugins.py"
WORKFLOWS = ROOT / ".github" / "workflows"
JUSTFILE = ROOT / "justfile"


def read(path):
    return path.read_text(encoding="utf-8")


def rust_constant(name):
    source = read(RELEASE_IDENTITY)
    match = re.search(
        rf'pub const {re.escape(name)}: &str =\s*"(.*?)";',
        source,
        re.DOTALL,
    )
    if match is None:
        raise AssertionError(f"{name} is not declared as a string constant")
    return "".join(match.group(1).split())


def application_signing_key():
    source = read(RUNTIME_MOD)
    match = re.search(
        r'pub const RELEASE_SIGNING_KEY: Option<&str> = Some\(\s*"(.*?)"\s*\);',
        source,
        re.DOTALL,
    )
    if match is None:
        raise AssertionError("RELEASE_SIGNING_KEY is not declared")
    return "".join(match.group(1).split())


class PluginDistributionContractTests(unittest.TestCase):
    def test_the_sdk_and_official_plugins_are_workspace_members(self):
        source = read(WORKSPACE)
        dependency = re.search(
            r'^bongocat-plugin-sdk\s*=\s*\{([^}]*)\}\s*$',
            source,
            re.MULTILINE,
        )
        self.assertIsNotNone(dependency, "the workspace must depend on the plugin SDK")
        self.assertIn('path = "crates/bongocat-plugin-sdk"', dependency.group(0))
        for path in [
            "crates/bongocat-plugin-sdk",
            "crates/bongocat-plugin-packaging",
            "plugins/input-source",
            "plugins/key-sounds",
            "plugins/pomodoro",
        ]:
            self.assertTrue((ROOT / path).is_dir(), path)
            self.assertIn(f'"{path}"', source)

    def test_plugin_releases_use_their_own_tag_namespace(self):
        workflow = read(WORKFLOWS / "plugin-release.yml")
        self.assertIn('tags:\n      - "plugins-v*"', workflow)
        self.assertNotIn('tags:\n      - "v*"', workflow)
        self.assertNotIn("--repository ayangweb/", workflow)
        self.assertIn(
            "initializing the first plugin release from current sources", workflow
        )
        self.assertIn("--latest=false", workflow)
        self.assertIn("plugin-catalog", workflow)
        self.assertIn("--release-url", workflow)
        self.assertIn("releases/download/$RELEASE_TAG", workflow)

        application = read(WORKFLOWS / "release.yml")
        self.assertIn('tags:\n      - "v*"', application)
        self.assertNotIn("plugins-v*", application)
        self.assertEqual(
            len(re.findall(r"group: release-publishing", application + workflow)), 2
        )

    def test_plugin_signing_is_separate_from_application_updates(self):
        public_key = base64.b64decode(rust_constant("PLUGIN_RELEASE_SIGNING_KEY"))
        self.assertTrue(
            public_key.startswith(b"untrusted comment: minisign public key: "),
            "the embedded value must be the base64-encoded Minisign public-key box",
        )
        self.assertNotEqual(
            rust_constant("PLUGIN_RELEASE_SIGNING_KEY"),
            application_signing_key(),
        )
        workflow = read(WORKFLOWS / "plugin-release.yml")
        self.assertIn("secrets.PLUGIN_SIGNING_PRIVATE_KEY", workflow)
        self.assertNotIn("secrets.SIGNING_PRIVATE_KEY", workflow)
        self.assertNotIn("secrets.RELEASE_SIGNING_KEY", workflow)

    def test_production_uses_one_fixed_signed_catalog_url(self):
        self.assertEqual(
            rust_constant("PLUGIN_CATALOG_URL"),
            "https://raw.githubusercontent.com/ayangweb/BongoCat/plugin-catalog/plugins.json",
        )
        source = read(APP_SOURCE)
        self.assertIn("PLUGIN_CATALOG_URL", source)
        self.assertNotIn("PLUGIN_REPOSITORY_OWNER", source)
        self.assertNotIn("PLUGIN_REPOSITORY_NAME", source)
        self.assertIn("BONGOCAT_PLUGIN_DEV_DIRECTORY", source)
        catalog = read(ROOT / "crates/bongocat-plugin-manager/src/catalog.rs")
        self.assertIn("validate_remote_assets", catalog)
        self.assertIn('starts_with("https://")', catalog)
        self.assertIn("raw.githubusercontent.com", catalog)

    def test_development_prepares_in_repository_plugins(self):
        justfile = read(JUSTFILE)
        compact = "".join(justfile.split())
        self.assertIn(
            "python3tools/dev_plugins.py--cargorun--locked-pbongocat-app--release",
            compact,
        )
        self.assertIn("plugin-buildtarget:", compact)
        self.assertIn("plugin-tests:", compact)
        script = read(DEV_PLUGINS)
        self.assertIn("BONGOCAT_PLUGIN_DEV_DIRECTORY", script)
        self.assertIn("BONGOCAT_PLUGIN_TEST_ROOT", script)
        self.assertIn("BONGOCAT_PLUGIN_SMOKE_REQUIRE_CARDS", script)
        self.assertIn("bongocat-plugin-packaging", script)
        self.assertNotIn("git clone", script)
        self.assertNotIn("/Users/", script)


if __name__ == "__main__":
    unittest.main()
