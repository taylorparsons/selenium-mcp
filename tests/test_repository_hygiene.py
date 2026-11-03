import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class PrivateRuntimeDefaults(unittest.TestCase):
    def test_private_runtime_and_credentials_are_ignored_but_examples_are_shareable(self):
        private = [
            '.selenium-mcp/profiles/personal/Default/Cookies',
            '.selenium-mcp/flows/my-bank.yaml',
            '.selenium-mcp/baselines/account.png',
            '.selenium-mcp/artifacts/recording/demo.mp4',
            '.env', '.env.production', 'credentials.json', 'secrets.local.yaml',
            'private-key.pem', 'client.key', '.codex/config.toml',
            'docs/private/notes.md', '.local/security-review.json',
        ]
        public = ['fixtures/flows/example.yaml', '.env.example', 'LICENSE', 'NOTICE']
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / '.gitignore').write_bytes((ROOT / '.gitignore').read_bytes())
            subprocess.run(['git', 'init', '-q', directory], check=True)
            for relative in private + public:
                result = subprocess.run(['git', 'check-ignore', '-q', relative], cwd=root)
                self.assertEqual(result.returncode == 0, relative in private, relative)


if __name__ == '__main__':
    unittest.main()
