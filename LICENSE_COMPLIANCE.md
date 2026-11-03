# Open-source license compliance review

**Reviewed:** 2026-11-03  
**Project:** `selenium-mcp`, copyright 2026 D. Taylor Parsons III  
**Project license:** Apache-2.0  
**Scope:** this repository's source release and the locked dependency inventory. This is a technical review, not a legal opinion or a certification that every possible downstream distribution is compliant.

## Decision for this repository

- **Publishing the current source repository:** no blocking license incompatibility was identified in the declared locked Rust dependency metadata. Keep the top-level `LICENSE`, `NOTICE`, `THIRD_PARTY_CREDITS.md` and `THIRD_PARTY_RUST.md` with the release.
- **Distributing compiled executables, containers, installers or bundled models:** **not cleared by this review**. Collect the complete license texts and required copyright/attribution notices for the actual binaries and included assets. Validate any third-party source availability or license conditions before release.
- **Code provenance:** license metadata alone cannot establish whether any repository source or fixture was copied from a third party. Check any reused snippets, assets, screenshots or adapted code before publication.

## Locked Rust dependencies

`THIRD_PARTY_RUST.md` lists **440 third-party locked packages** and their crate-published SPDX expressions, authors when declared, and repository links; `Cargo.lock` also contains the project's own package. These include platform-specific, build and transitive crates, some of which may not be linked into a particular target binary.

The declarations are predominantly Apache-2.0, MIT, BSD, ISC and related permissive expressions. The following need express treatment:

| Item | License in locked inventory | Review and required release treatment |
| --- | --- | --- |
| `option-ext 0.2.0`, upstream Simon Ochsenreither | MPL-2.0 | **Reviewed conditional dependency.** MPL-2.0 has file-level copyleft, not a requirement to relicense the whole Apache-2.0 application. If its covered code is distributed in executable form, make its exact corresponding source available and retain the MPL license and notices. If the crate was modified, make the modified covered files available under MPL-2.0. [Package](https://crates.io/crates/option-ext/0.2.0), [MPL 2.0](https://www.mozilla.org/en-US/MPL/2.0/) |
| 18 ICU4X-family crates, plus `unicode-ident` | Unicode-3.0 | A permissive software/data license. Preserve the applicable Unicode copyright and permission notices in any redistribution. [Unicode licensing](https://unicode.org/policies/licensing_policy.html) |
| `webpki-roots 1.0.10`, `webpki-root-certs 1.0.10` | CDLA-Permissive-2.0 | The certificate data is separately licensed. When sharing the covered data, provide the text of the CDLA agreement. [License](https://cdla.dev/permissive-2-0/) |
| `r-efi 5.3.0` and `6.0.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | Permissive alternatives appear in the crate metadata. Preserve the applicable chosen-license notices when redistributed; do not misrepresent LGPL as the only option. |
| `aws-lc-sys 0.46.0` | Multiple ISC, Apache-2.0, MIT, BSD-3-Clause and MIT-0 components, combined with AND | Its composite license expression makes a full license-text and copyright-notice inventory especially important for binary packages. |
| Other crates with MIT/BSD/ISC/Apache/Unicode or `WITH` conditions | See exact `THIRD_PARTY_RUST.md` entries | Keep required notices and license texts for included code. Some crates list multiple `OR` choices; an SPDX expression is not itself the full notice. |

There are no declared GPL-only or AGPL-only Rust package licenses in this snapshot's 440-entry report. This is not a guarantee about native dynamic libraries, binaries fetched during installation, or future dependency changes.

## Other applications, binaries, services and models

| Component | How it is used | Source-only release | If bundled or redistributed later |
| --- | --- | --- | --- |
| [Selenium/WebDriver](https://www.selenium.dev/documentation/about/copyright/) | Browser automation protocol/ecosystem | Upstream Selenium project code is not included as an application dependency; `thirtyfour` is the separately licensed Rust WebDriver client | Retain Apache-2.0 obligations if Selenium code is actually copied or packaged |
| [Thirtyfour](https://github.com/stevepryde/thirtyfour) | Compiled Rust client | Included in the Rust dependency list | MIT OR Apache-2.0; preserve applicable license and copyright notices |
| [Google Chrome](https://policies.google.com/terms) and [Chromium](https://github.com/chromium/chromium/blob/main/LICENSE) | Browser installed separately by the user/installer | No Chrome/Chromium binary is stored in this repository | Google Chrome is not generically relicensable as open source. Review Google's terms before repackaging it. Chromium is primarily BSD-3-Clause with numerous third-party licenses; audit the exact distributed build |
| ChromeDriver | Installed/provisioned separately as a WebDriver component | No driver binary is stored in this repository | Review the exact driver's license and upstream notices before including binaries |
| [Chroma](https://github.com/chroma-core/chroma) | Optional server or `uvx` download at runtime (currently `chromadb==1.5.9`) | Apache-2.0 Chroma project is credited; no Chroma executable or Python dependency tree is bundled | If bundled, audit its complete Python/native transitive distribution and include all relevant notices |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) / FastEmbed | Native inference dependencies reached via Rust crates | Rust `ort` and FastEmbed appear in the inventory | Ensure native ONNX Runtime assets and their corresponding notices match the distributed build; ONNX Runtime itself is MIT |
| [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) | Model downloaded on first use | Model weights are not committed; model card identifies Apache-2.0 | If weights/config/tokenizer assets are bundled, include model license, attribution and license notices for any additional assets |
| [FFmpeg](https://ffmpeg.org/legal.html) | Separate executable on PATH, local cache or an explicitly selected verified download | No FFmpeg binary is bundled. Rust `ffmpeg-sidecar` is independently MIT | FFmpeg is normally LGPL-2.1-or-later; some builds use GPL-2.0-or-later and `--enable-nonfree` builds may be unredistributable. Inspect the exact build and comply with source/license obligations if shipping it |
| `uv`, Rust toolchain, GitHub Actions and Claude Code | Optional tooling, build/CI or client integration | Not delivered as part of the `selenium-mcp` source package | Review each separately if a future installer begins repackaging it |

## Required checks for a source-only publication

1. Keep the unmodified full Apache-2.0 license in `LICENSE`, and the author's correct 2026 copyright and project notice in `NOTICE`.
2. Keep the main upstream credits in `THIRD_PARTY_CREDITS.md`, and the complete resolved-crate table in `THIRD_PARTY_RUST.md`.
3. Run `python3 scripts/audit_license_inventory.py` after dependency changes. It compares the inventory with `Cargo.lock` and rejects unreviewed license identifiers or new MPL-2.0 packages. It is a regression gate, **not** proof of fulfillment of redistribution obligations.
4. Regenerate the crate inventory after each lockfile change with `python3 scripts/generate_rust_credits.py --output THIRD_PARTY_RUST.md`. Test freshness using `--check`.
5. Do not add external binaries, model assets, vendor directories, screenshots or copied third-party code without an asset-specific license review.

## Additional requirements before distributing compiled releases

1. Determine what is **actually included** in each release artifact (Rust binary, shared libraries, embedded data, optional FFmpeg/Chrome binaries, models, and packaged installers).
2. Generate the **full license texts and preserved copyright notices** for those included crates/assets. The current Markdown dependency table names licenses but **does not include their complete terms**. A tool such as [cargo-about](https://docs.rs/cargo-about/latest/cargo_about/) can collect Rust crate license texts. Verify its output and all exceptions rather than treating the tool as legal approval.
3. For any build that includes `option-ext`, provide the exact MPL-2.0 license text and a documented way to retrieve the corresponding source archive, e.g. [the 0.2.0 crate](https://crates.io/crates/option-ext/0.2.0). Supply modified covered-file sources under MPL-2.0 if you ever modify them.
4. Include the applicable Unicode-3.0 notices and CDLA-Permissive-2.0 text if corresponding code/data are present. The multi-license native crypto stack requires its own copyright/notices check.
5. For every FFmpeg binary, inspect `ffmpeg -version` and the build's `configuration:` flags; verify GPL and nonfree switches, then follow [FFmpeg's distribution checklist](https://ffmpeg.org/legal.html). Do not assume a generic binary is redistributable.
6. Verify the license terms of every downloaded model/asset and package the needed notices with the artifact. Do not bundle Google Chrome or ChromeDriver on the assumption that Selenium's Apache-2.0 license covers them.
7. Publish the final attribution and license-text bundle **alongside every binary artifact** and validate it against that artifact's exact dependency/source versions.

## Review limitations

This review uses the declared `Cargo.lock`, generated `THIRD_PARTY_RUST.md`, source tree and linked upstream project/license pages. It did not decompile binary builds, inspect every upstream crate source file or embedded notice, analyze third-party asset provenance, or examine packages distributed outside this GitHub repository. There are no GitHub release artifacts in the repository at the time of this review. If there are downloadable binaries distributed elsewhere, audit those artifacts separately.

For a commercial binary release or an uncertain license exception, obtain a qualified open-source licensing/legal review.
