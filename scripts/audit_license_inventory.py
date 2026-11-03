#!/usr/bin/env python3
"""Check that the locked Rust dependency inventory has reviewed license expressions.

This is a metadata and policy regression check, not a legal compliance
certification. It does not collect required license texts or analyze binaries.
"""
from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

# SPDX expressions in current Cargo metadata use AND/OR/WITH, parentheses,
# and some legacy dual-license spellings with '/'. A new identifier must be
# reviewed and explicitly added to this set before CI will pass.
REVIEWED_IDS = {
    "0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause",
    "BSL-1.0", "CC0-1.0", "CDLA-Permissive-2.0", "ISC",
    "MIT", "MIT-0", "MPL-2.0", "NCSA", "Unlicense",
    "Unicode-3.0", "Zlib", "LLVM-exception",
    "LGPL-2.1-or-later",
}

# MPL-2.0 is permitted in an Apache-2.0 larger work, but executable
# distribution triggers covered-source and notice obligations. Refer to
# LICENSE_COMPLIANCE.md before releasing compiled executables.
REVIEWED_MPL_PACKAGES = {("option-ext", "0.2.0")}

# Both published versions offer MIT and Apache-2.0 alternatives. Do not
# approve an unrelated LGPL crate solely because an OR expression is present.
REVIEWED_LGPL_PACKAGES = {("r-efi", "5.3.0"), ("r-efi", "6.0.0")}

ROW = re.compile(
    r"^\| \[`(?P<name>[^`]+)`\]"
    r"\(https://crates\.io/crates/[^)]*\) \| "
    r"(?P<version>[^|]+) \| (?P<authors>.*?) \| "
    r"(?P<license>.*?) \| "
)


def read_lock(path: Path) -> dict[tuple[str, str], dict]:
    values = tomllib.loads(path.read_text())["package"]
    return {
        (pkg["name"], pkg["version"]): pkg
        for pkg in values
        if pkg.get("source", "").startswith("registry+")
    }


def read_credits(path: Path) -> tuple[dict[tuple[str, str], str], list[str]]:
    found = {}
    errors = []
    for line_number, line in enumerate(path.read_text().splitlines(), 1):
        if not line.startswith("| [`"):
            continue
        match = ROW.match(line)
        if not match:
            errors.append(f"Malformed credits row {line_number}")
            continue
        item = (match["name"], match["version"].strip())
        if item in found:
            errors.append(f"Duplicate credits row: {item[0]} {item[1]}")
        found[item] = match["license"].strip()
    return found, errors


def check_license(name: str, version: str, expression: str) -> list[str]:
    label = f"{name} {version}"
    if not expression or "MISSING" in expression.upper() or "UNKNOWN" in expression.upper():
        return [f"{label}: missing license metadata"]
    normalized = re.sub(r"\s*/\s*", " OR ", expression)
    tokens = set(re.findall(r"[A-Za-z0-9][A-Za-z0-9.\-]*", normalized))
    operators = {"AND", "OR", "WITH"}
    unknown = tokens - operators - REVIEWED_IDS
    errors = [f"{label}: unreviewed license identifier(s): {', '.join(sorted(unknown))}"] if unknown else []
    if "MPL-2.0" in tokens and (name, version) not in REVIEWED_MPL_PACKAGES:
        errors.append(f"{label}: MPL-2.0 package needs its own release review")
    if "LGPL-2.1-or-later" in tokens and (\n        (name, version) not in REVIEWED_LGPL_PACKAGES\n        or not ({"MIT", "Apache-2.0"} & tokens)\n        or "OR" not in tokens\n    ):\n        errors.append(f"{label}: LGPL choice requires individual review")
    return errors


def audit(lock: dict, credits: dict, parse_errors: list[str] | None = None) -> list[str]:
    problems = list(parse_errors or [])
    for name, version in sorted(lock.keys() - credits.keys()):
        problems.append(f"Missing credits entry: {name} {version}")
    for name, version in sorted(credits.keys() - lock.keys()):
        problems.append(f"Stale credits entry: {name} {version}")
    for (name, version), expression in sorted(credits.items()):
        if (name, version) in lock:
            problems.extend(check_license(name, version, expression))
    return problems


def main() -> int:
    lock = read_lock(ROOT / "Cargo.lock")
    credits, parse_errors = read_credits(ROOT / "THIRD_PARTY_RUST.md")
    errors = audit(lock, credits, parse_errors)
    for error in errors:
        print("FAIL:", error, file=sys.stderr)
    if errors:
        return 1
    print(f"License metadata check passed for {len(lock)} locked third-party Rust packages.")
    print("Reviewed exceptions include option-ext 0.2.0 (MPL-2.0).")
    print("Binary distributions require separate license-text and source-access checks.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
