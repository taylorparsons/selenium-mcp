import contextlib
import importlib.util
import io
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('security_check', pathlib.Path(__file__).resolve().parents[1] / 'scripts/security_check.py')
security = importlib.util.module_from_spec(spec)
spec.loader.exec_module(security)


class AdvisoryHandling(unittest.TestCase):
    def run_audit(self, advisory, result=None):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / 'Cargo.lock').write_text('[[package]]\nname="fixture"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n')
            batch = {'results': [result or {'vulns': [{'id': 'FIXTURE-1'}]}]}
            responses = [io.BytesIO(json.dumps(batch).encode()), io.BytesIO(json.dumps(advisory).encode())]
            with patch.object(security, 'ROOT', root), patch.object(security.urllib.request, 'urlopen', side_effect=responses), contextlib.redirect_stdout(io.StringIO()):
                return security.dependency_checks()

    def test_vulnerability_match_fails(self):
        self.assertTrue(self.run_audit({'affected': [{'database_specific': {}}]}))

    def test_unsoundness_is_not_downgraded_to_a_maintenance_warning(self):
        self.assertTrue(self.run_audit({'affected': [{'database_specific': {'informational': 'unsound'}}]}))

    def test_unmaintained_only_is_a_warning(self):
        self.assertFalse(self.run_audit({'affected': [{'database_specific': {'informational': 'unmaintained'}}]}))

    def test_withdrawn_advisory_does_not_block_release(self):
        self.assertFalse(self.run_audit({'withdrawn': '2026-01-01T00:00:00Z'}))

    def test_malformed_advisory_does_not_silently_pass(self):
        self.assertTrue(self.run_audit({'affected': []}))

    def test_pagination_requires_a_complete_audit(self):
        with self.assertRaises(RuntimeError):
            self.run_audit({}, {'next_page_token': 'another-page'})



class PublicationHygiene(unittest.TestCase):
    def test_only_the_explicit_public_claude_skill_is_allowed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            public_skill = '.claude/skills/selenium-mcp/SKILL.md'
            public_path = root / public_skill
            public_path.parent.mkdir(parents=True)
            public_path.write_text('# Published Claude skill')
            private_skill = '.claude/skills/selenium-mcp/private.md'
            (root / private_skill).write_text('private')

            with patch.object(security, 'ROOT', root), patch.object(
                security, 'candidates', return_value=[public_skill]
            ), contextlib.redirect_stdout(io.StringIO()):
                self.assertFalse(security.file_checks())

            with patch.object(security, 'ROOT', root), patch.object(
                security, 'candidates', return_value=[public_skill, private_skill]
            ), contextlib.redirect_stdout(io.StringIO()):
                self.assertTrue(security.file_checks())

    def test_public_skill_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source = root / 'source.md'
            source.write_text('example')
            public_skill = '.claude/skills/selenium-mcp/SKILL.md'
            public_path = root / public_skill
            public_path.parent.mkdir(parents=True)
            public_path.symlink_to(source)
            with patch.object(security, 'ROOT', root), patch.object(
                security, 'candidates', return_value=[public_skill]
            ), contextlib.redirect_stdout(io.StringIO()):
                self.assertTrue(security.file_checks())


if __name__ == '__main__':
    unittest.main()
