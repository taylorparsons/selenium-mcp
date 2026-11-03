"""Regression tests for the third-party Rust license metadata gate."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "audit_license_inventory", ROOT / "scripts/audit_license_inventory.py"
)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class LicenseAuditTests(unittest.TestCase):
    def test_permissive_and_reviewed_exception(self):
        self.assertEqual(audit.check_license("tokio", "1", "MIT"), [])
        self.assertEqual(audit.check_license("option-ext", "0.2.0", "MPL-2.0"), [])
        self.assertEqual(audit.check_license("icu_collections", "2.3.0", "Unicode-3.0"), [])
        self.assertEqual(audit.check_license("webpki-roots", "1.0.10", "CDLA-Permissive-2.0"), [])
        self.assertEqual(
            audit.check_license("r-efi", "6.0.0", "MIT OR Apache-2.0 OR LGPL-2.1-or-later"), []
        )
        self.assertEqual(
            audit.check_license("aws-lc", "1", "ISC AND (Apache-2.0 OR ISC) AND BSD-3-Clause"), []
        )

    def test_reject_new_mpl_crate(self):
        errors = audit.check_license("new-component", "1.0.0", "MPL-2.0")
        self.assertTrue(any("needs its own release review" in error for error in errors))

    def test_reject_unreviewed_gpl_and_missing_metadata(self):
        self.assertTrue(audit.check_license("new", "1", "GPL-3.0-only"))
        self.assertTrue(audit.check_license("new", "1", "AGPL-3.0-only"))
        self.assertTrue(audit.check_license("new", "1", "UNKNOWN"))
        self.assertTrue(audit.check_license("new", "1", "LGPL-2.1-or-later"))
        self.assertTrue(audit.check_license("new", "1", "MIT OR LGPL-2.1-or-later"))
        self.assertTrue(audit.check_license("r-efi", "9", "MIT OR LGPL-2.1-or-later"))

    def test_accept_legacy_dual_license_format(self):
        self.assertEqual(audit.check_license("old", "1", "MIT/Apache-2.0"), [])
        self.assertEqual(audit.check_license("old", "1", "Apache-2.0 / MIT"), [])

    def test_detect_missing_and_extra_lock_entries(self):
        lock = {("a", "1"): {}, ("b", "2"): {}}
        credits = {("a", "1"): "MIT", ("stale", "9"): "MIT"}
        problems = audit.audit(lock, credits)
        self.assertEqual(len(problems), 2)
        self.assertTrue(any("Missing credits entry: b 2" in x for x in problems))
        self.assertTrue(any("Stale credits entry: stale 9" in x for x in problems))

    def test_read_inventory_and_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_text(
                'version = 4\n[[package]]\nname = "project"\nversion = "0.1.0"\n'
                '[[package]]\nname = "tokio"\nversion = "1.0.0"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            )
            (root / "THIRD_PARTY_RUST.md").write_text(
                '# Credits\n| Crate | Version | Authors | License | Project |\n'
                '| --- | --- | --- | --- | --- |\n'
                '| [`tokio`](https://crates.io/crates/tokio/1.0.0) '
                '| 1.0.0 | Tokio Contributors | MIT | [upstream](https://tokio.rs) |\n'
            )
            lock = audit.read_lock(root / "Cargo.lock")
            credits, parse_errors = audit.read_credits(root / "THIRD_PARTY_RUST.md")
            self.assertEqual(set(lock), {("tokio", "1.0.0")})
            self.assertEqual(audit.audit(lock, credits, parse_errors), [])

    def test_current_inventory_matches_lock(self):
        lock = audit.read_lock(ROOT / "Cargo.lock")
        credits, parse_errors = audit.read_credits(ROOT / "THIRD_PARTY_RUST.md")
        self.assertEqual(audit.audit(lock, credits, parse_errors), [])


if __name__ == "__main__":
    unittest.main()
