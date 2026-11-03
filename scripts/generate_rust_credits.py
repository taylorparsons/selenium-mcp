#!/usr/bin/env python3
"""Generate a Rust dependency credit inventory from locked Cargo metadata."""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote


ROOT = Path(__file__).resolve().parents[1]


def cell(value: str) -> str:
    return " ".join(value.replace("|", r"\|").split())


def link(url: str) -> str:
    if url.startswith("https://") or url.startswith("http://"):
        return f"[upstream]({url.replace(')', '%29')})"
    return "not provided"


def render(metadata: dict) -> tuple[str, list[str]]:
    packages = [
        item for item in metadata["packages"]
        if item.get("source") and item["name"] != "selenium-mcp"
    ]
    packages.sort(key=lambda p: (p["name"].casefold(), p["version"]))
    missing = []
    rows = []
    for package in packages:
        name = package["name"]
        version = package["version"]
        upstream = package.get("repository") or package.get("homepage") or ""
        authors = []
        for author in package.get("authors") or []:
            clean = re.sub(r"\s*<[^>]*>", "", author).strip()
            if clean:
                authors.append(clean)
        author_text = ", ".join(authors) if authors else "Not declared in crate metadata"
        license_id = package.get("license")
        if license_id:
            license_text = license_id
        elif package.get("license_file"):
            license_text = "See package LICENSE file"
        else:
            license_text = "**MISSING: review required**"
            missing.append(f"{name} {version}")
        crate_url = (
            "https://crates.io/crates/"
            + quote(name, safe="")
            + "/"
            + quote(version, safe="")
        )
        rows.append(
            f"| [`{cell(name)}`]({crate_url}) | {cell(version)} | "
            f"{cell(author_text)} | {cell(license_text)} | {link(upstream)} |"
        )
    header = """# Rust dependency credits

This list was generated from the project's resolved Cargo dependency graph
(`cargo metadata --locked`). It covers direct and transitive third-party Rust
crates, including platform-specific dependencies. The named authors are those
declared in crate metadata. A crate's maintainers or copyright owners may
differ. Visit the linked upstream project for license texts and contributors.

The project's own license is in `LICENSE`. See `THIRD_PARTY_CREDITS.md`
for the upstream browser, MCP, model, database and media projects.

| Crate | Locked version | Authors listed by the crate | SPDX license / license file | Project |
| --- | --- | --- | --- | --- |
"""
    return header + "\n".join(rows) + "\n", missing


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="THIRD_PARTY_RUST.md")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--check", action="store_true",
                        help="fail if the output differs; do not write")
    args = parser.parse_args()

    command = ["cargo", "metadata", "--locked", "--format-version", "1"]
    if args.offline:
        command.append("--offline")
    try:
        result = subprocess.run(command, cwd=ROOT, capture_output=True,
                                text=True, check=True)
    except (FileNotFoundError, subprocess.CalledProcessError) as exc:
        print(f"Unable to read Cargo metadata: {exc}", file=sys.stderr)
        return 2
    report, missing = render(json.loads(result.stdout))
    output = (ROOT / args.output).resolve()
    if args.check:
        if not output.exists() or output.read_text() != report:
            print(f"Stale dependency credits: {output}", file=sys.stderr)
            return 1
    else:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(report)

    if missing:
        print("Crates without declared license metadata:", file=sys.stderr)
        for item in missing:
            print(f"  {item}", file=sys.stderr)
        return 2

    print(f"Dependency credits verified: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
