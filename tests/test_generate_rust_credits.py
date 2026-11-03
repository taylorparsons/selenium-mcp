import importlib.util
import pathlib
import shutil
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "generate_rust_credits", ROOT / "scripts/generate_rust_credits.py"
)
credits = importlib.util.module_from_spec(spec)
spec.loader.exec_module(credits)


class DependencyCredits(unittest.TestCase):
    def test_locked_crates_have_owner_license_and_source(self):
        inventory = {
            "packages": [
                {"name": "selenium-mcp", "version": "0.1.0", "source": None},
                {
                    "name": "z-example", "version": "1.2.3", "source": "registry+crates.io",
                    "authors": ["Example Maintainer <example@invalid.test>"],
                    "license": "MIT OR Apache-2.0",
                    "repository": "https://example.org/z-example",
                },
                {
                    "name": "a-example", "version": "0.5.0", "source": "registry+crates.io",
                    "authors": [], "license_file": "LICENSE", "repository": None,
                },
            ],
        }
        report, missing = credits.render(inventory)
        self.assertFalse(missing)
        self.assertLess(report.index("a-example"), report.index("z-example"))
        self.assertIn("Example Maintainer", report)
        self.assertNotIn("example@invalid.test", report)
        self.assertIn("MIT OR Apache-2.0", report)
        self.assertIn("See package LICENSE file", report)
        self.assertIn("https://crates.io/crates/z-example/1.2.3", report)
        self.assertIn("[upstream](https://example.org/z-example)", report)
        self.assertNotIn("selenium-mcp | 0.1.0", report)

    def test_missing_license_metadata_requires_review(self):
        report, missing = credits.render({
            "packages": [
                {
                    "name": "unlicensed-example", "version": "1.0.0",
                    "source": "registry+crates.io",
                }
            ],
        })
        self.assertEqual(missing, ["unlicensed-example 1.0.0"])
        self.assertIn("MISSING: review required", report)

    def test_markdown_special_characters_are_escaped(self):
        self.assertEqual(credits.cell("Project | Owner"), r"Project \| Owner")
        self.assertEqual(credits.link("file:///local/repo"), "not provided")

    def test_snapshot_publication_script_syntax_when_present(self):
        script = ROOT / "scripts/prepare_public_snapshot.sh"
        if script.exists() and shutil.which("bash"):
            subprocess.run(["bash", "-n", str(script)], check=True)


if __name__ == "__main__":
    unittest.main()
