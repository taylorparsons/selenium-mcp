# Open-source credits

`selenium-mcp` is an independent project created by D. Taylor Parsons III, licensed under Apache-2.0. It is built with and around work maintained by open-source communities. Thank you to the developers, maintainers and contributors listed below.

Names and links identify the upstream project or its maintainers. They do not imply that those projects endorse or maintain `selenium-mcp`.

## Browser automation and MCP

| Project | Maintainers / project owners | Contribution to this project | Upstream | License |
| --- | --- | --- | --- | --- |
| [Selenium](https://www.selenium.dev/) | Selenium project contributors | Browser automation ecosystem and WebDriver conventions | [SeleniumHQ/selenium](https://github.com/SeleniumHQ/selenium) | [Apache-2.0](https://github.com/SeleniumHQ/selenium/blob/trunk/LICENSE) |
| [Thirtyfour](https://github.com/stevepryde/thirtyfour) | Steve Pryde, Vrtgs and contributors | Rust WebDriver client | [stevepryde/thirtyfour](https://github.com/stevepryde/thirtyfour) | MIT OR Apache-2.0 |
| [RMCP, the official MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) | Model Context Protocol Rust SDK contributors | MCP protocol server and tools | [modelcontextprotocol/rust-sdk](https://github.com/modelcontextprotocol/rust-sdk) | Apache-2.0 |
| [Tokio](https://tokio.rs/) | Tokio contributors | Async runtime | [tokio-rs/tokio](https://github.com/tokio-rs/tokio) | MIT |
| [Reqwest](https://github.com/seanmonstar/reqwest) | Sean McArthur and contributors | HTTP client | [seanmonstar/reqwest](https://github.com/seanmonstar/reqwest) | MIT OR Apache-2.0 |

## Semantic memory and models

| Project | Maintainers / project owners | Contribution to this project | Upstream | License |
| --- | --- | --- | --- | --- |
| [Chroma](https://www.trychroma.com/) | Chroma contributors | Optional vector storage and search | [chroma-core/chroma](https://github.com/chroma-core/chroma) | Apache-2.0 |
| [FastEmbed-rs](https://github.com/Anush008/fastembed-rs) | Anush and contributors | Local embedding inference | [Anush008/fastembed-rs](https://github.com/Anush008/fastembed-rs) | Apache-2.0 |
| [ONNX Runtime](https://onnxruntime.ai/) | Microsoft and ONNX Runtime contributors | Model inference via FastEmbed | [microsoft/onnxruntime](https://github.com/microsoft/onnxruntime) | MIT |
| [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) | Sentence Transformers contributors | Downloaded text embedding model | [Model card](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) | Apache-2.0 |

## Media and supporting Rust libraries

| Project | Maintainers / project owners | Contribution to this project | Upstream | License |
| --- | --- | --- | --- | --- |
| [FFmpeg](https://ffmpeg.org/) | FFmpeg contributors | Optional external video encoding tool | [FFmpeg/FFmpeg](https://github.com/FFmpeg/FFmpeg) | LGPL-2.1-or-later by default; some builds GPL-2.0-or-later. Check the actual binary's build configuration |
| [ffmpeg-sidecar](https://github.com/nathanbabcock/ffmpeg-sidecar) | Nathan Babcock and contributors | Rust wrapper for the FFmpeg executable | [nathanbabcock/ffmpeg-sidecar](https://github.com/nathanbabcock/ffmpeg-sidecar) | MIT |
| [Serde](https://serde.rs/) | Erick Tryzelaar, David Tolnay and contributors | Rust serialization framework | [serde-rs/serde](https://github.com/serde-rs/serde) | MIT OR Apache-2.0 |
| [serde-saphyr](https://github.com/bourumir-wyngs/serde-saphyr) | serde-saphyr contributors | YAML parsing and serialization | [bourumir-wyngs/serde-saphyr](https://github.com/bourumir-wyngs/serde-saphyr) | MIT OR Apache-2.0 |
| [SHA-2 / RustCrypto](https://github.com/RustCrypto/hashes) | RustCrypto Developers | SHA-256 checksum verification | [RustCrypto/hashes](https://github.com/RustCrypto/hashes) | MIT OR Apache-2.0 |
| [Tracing](https://github.com/tokio-rs/tracing) | Tokio project contributors | Structured diagnostics | [tokio-rs/tracing](https://github.com/tokio-rs/tracing) | MIT |
| [dirs-rs](https://codeberg.org/dirs/dirs-rs) | dirs-rs contributors | Platform-specific directories | [dirs-rs](https://codeberg.org/dirs/dirs-rs) | MIT OR Apache-2.0 |

Other direct Rust libraries, including `schemars`, `serde_json`, `thiserror`, `anyhow` and `image`, and all transitive libraries have their own copyright holders, contributors and license terms.

## Full dependency attribution

Run the included script after changing dependencies:

```sh
python3 scripts/generate_rust_credits.py --output THIRD_PARTY_RUST.md
```

It obtains crate names, exact resolved versions, authors, repository links and SPDX license expressions from `cargo metadata --locked`. The generated report includes all resolved third-party Rust crates, not only the projects highlighted above. Keep the generated report with each public release. Missing license or author metadata must be investigated rather than guessed.

## Licensing and non-endorsement

This project's `LICENSE` and `NOTICE` apply to `selenium-mcp` itself. Third-party code, dependencies, binaries and model weights retain their own licenses and notices. See each upstream link and the generated dependency inventory for their terms. FFmpeg's applicable license depends on how the executable was built; Chrome is not an open-source dependency merely because ChromeDriver or Chromium components are available under open-source licenses.

Credits are an acknowledgment, not an assertion of affiliation or endorsement and not a substitute for any license files, notices, attribution or redistribution obligations required by the upstream projects.

## License compliance and distribution

See [LICENSE_COMPLIANCE.md](LICENSE_COMPLIANCE.md) for the reviewed Rust license
exceptions (including `option-ext` under MPL-2.0), separately installed or
downloaded applications, model weights and the obligations for compiled releases.
This credits page and `THIRD_PARTY_RUST.md` identify dependencies but do not
replace their full license texts and copyright notices in a binary distribution.
