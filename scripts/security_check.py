#!/usr/bin/env python3
"""Read-only release checks. Invoke through ./scripts/owl security-scan."""
import argparse
import fnmatch
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import urllib.request


ROOT = Path(__file__).resolve().parents[1]


def candidates():
    result = subprocess.run(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'],
        cwd=ROOT, check=True, capture_output=True,
    )
    return sorted(set(p for p in result.stdout.decode().split('\0') if p))


def file_checks():
    problems = []
    for name in candidates():
        path = Path(name)
        # This one tracked Claude skill is intentionally public (see .gitignore).
        public_skill = name == '.claude/skills/selenium-mcp/SKILL.md'
        sensitive = name.startswith(('.selenium-mcp/', '.local/', '.codex/', '.claude/', 'docs/private/', 'docs/personal/')) and not public_skill
        sensitive |= path.name == '.env' or (path.name.startswith('.env.') and path.name != '.env.example')
        sensitive |= any(fnmatch.fnmatch(path.name.lower(), pattern) for pattern in (
            '*.pem', '*.key', '*.p12', '*.pfx', 'credentials*.json', 'secrets*.json', 'secrets*.yaml', 'secrets*.yml',
        ))
        sensitive |= any(part in ('target', '__pycache__', 'node_modules') for part in path.parts)
        sensitive |= path.name == '.DS_Store' or path.name.endswith('.rs.bk')
        if sensitive or (ROOT / name).is_symlink():
            problems.append(name)
    for name in problems:
        print(f'FAIL: private or symlinked file eligible for publication: {name}')
    print(f'File hygiene: {len(problems)} problem(s)')
    return bool(problems)


def dependency_checks():
    packages = [p for p in tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
                if p.get('source', '').startswith('registry+')]
    queries = [{'package': {'name': p['name'], 'ecosystem': 'crates.io'}, 'version': p['version']}
               for p in packages]
    body = json.dumps({'queries': queries}).encode()
    request = urllib.request.Request('https://api.osv.dev/v1/querybatch', data=body,
                                     headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=45) as response:
        results = json.load(response)['results']
    if len(results) != len(packages):
        raise RuntimeError('OSV returned an incomplete response')
    failed = False
    cache = {}
    for package, result in zip(packages, results):
        if result.get('next_page_token'):
            raise RuntimeError('OSV response was paginated; a complete audit is required')
        for match in result.get('vulns', []):
            advisory_id = match['id']
            if advisory_id not in cache:
                with urllib.request.urlopen('https://api.osv.dev/v1/vulns/' + advisory_id, timeout=30) as response:
                    cache[advisory_id] = json.load(response)
            advisory = cache[advisory_id]
            if advisory.get('withdrawn'):
                continue
            affected = advisory.get('affected', [])
            informational = bool(affected) and all(
                a.get('database_specific', {}).get('informational') == 'unmaintained'
                for a in affected
            )
            print(f'{"WARN" if informational else "FAIL"}: {package["name"]}@{package["version"]}: {advisory_id}: {advisory.get("summary", "advisory")}')
            failed |= not informational
    print(f'Dependency audit: {len(packages)} locked packages; {"FAILED" if failed else "no vulnerability matches"}')
    return failed


def secret_checks():
    binary = os.environ.get('GITLEAKS_BIN') or shutil.which('gitleaks')
    if not binary:
        raise RuntimeError('Install Gitleaks 8.30.1 or set GITLEAKS_BIN; scan cannot be skipped')
    args = [binary, '--redact', '--no-banner', '--ignore-gitleaks-allow',
            '--config', str(ROOT / '.gitleaks.toml')]

    with tempfile.TemporaryDirectory(prefix='selenium-gitleaks-reports-') as reports, \
         tempfile.TemporaryDirectory(prefix='selenium-publication-scan-') as directory:
        history_report = Path(reports) / 'history.json'
        history = subprocess.run([
            *args, '--report-format', 'json', '--report-path', str(history_report),
            'git', '--log-opts=--all', str(ROOT)
        ], cwd=ROOT)

        for name in candidates():
            source = ROOT / name
            if source.is_symlink():
                raise RuntimeError(f'Refusing to follow publication symlink: {name}')
            if source.is_file():
                destination = Path(directory) / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)

        current_report = Path(reports) / 'current.json'
        current = subprocess.run([
            *args, '--report-format', 'json', '--report-path', str(current_report),
            'dir', directory
        ], cwd=ROOT)

        # Print only finding metadata for remediation, never matched text or secrets.
        for label, report in [('history', history_report), ('current', current_report)]:
            if report.exists():
                for finding in json.loads(report.read_text()):
                    file_name = finding.get('File', '')
                    if label == 'current':
                        file_name = os.path.relpath(file_name, directory)
                    print(f'FAIL: Gitleaks {label}: {finding.get("RuleID", "unknown")} at '
                          f'{file_name}:{finding.get("StartLine", "?")}')
        return history.returncode != 0 or current.returncode != 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--files', action='store_true')
    parser.add_argument('--dependencies', action='store_true')
    parser.add_argument('--secrets', action='store_true')
    args = parser.parse_args()
    all_checks = not any((args.files, args.dependencies, args.secrets))
    failed = False
    for enabled, check in ((args.files or all_checks, file_checks),
                           (args.dependencies or all_checks, dependency_checks),
                           (args.secrets or all_checks, secret_checks)):
        if enabled:
            try:
                failed |= check()
            except Exception as error:
                print(f'FAIL: {check.__name__}: {error}')
                failed = True
    return int(failed)


if __name__ == '__main__':
    raise SystemExit(main())
