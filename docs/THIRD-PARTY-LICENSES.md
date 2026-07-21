# Third-Party Licenses

AO2 is licensed as `Apache-2.0`. See `../LICENSE`.

This notice is generated from local Cargo package metadata for the current
workspace dependency graph. It is an engineering inventory, not legal advice.
Keep this file current when dependencies change.

Audit command:

```sh
cargo metadata --format-version 1 | jq -r '.packages[] | [.name,.version,(.license // "NOASSERTION")] | @tsv' | sort -u
```

## License Families Present

- `MIT`
- `Apache-2.0`
- `Apache-2.0 WITH LLVM-exception`
- `0BSD`
- `BSL-1.0`
- `Unicode-3.0`
- `Unlicense`
- `Zlib`

Most third-party dependencies are licensed as `MIT OR Apache-2.0` or equivalent. The
non-MIT/Apache-only license families currently appear through these packages:

| License expression | Packages |
| --- | --- |
| `0BSD OR MIT OR Apache-2.0` | `adler2` |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `linux-raw-sys`, `rustix`, `wasi`, `wit-bindgen-rt` |
| `Apache-2.0 OR BSL-1.0` | `ryu` |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | `unicode-ident` |
| `MIT OR Zlib OR Apache-2.0` | `miniz_oxide` |
| `Unlicense OR MIT` | `memchr`, `winapi-util` |
| `Unlicense/MIT` | `same-file`, `walkdir` |
| `Zlib` | `zmij` |

## Full Cargo License Inventory

| Package | Version | License |
| --- | --- | --- |
| `adler2` | `2.0.1` | `0BSD OR MIT OR Apache-2.0` |
| `android_system_properties` | `0.1.5` | `MIT/Apache-2.0` |
| `anstream` | `0.6.21` | `MIT OR Apache-2.0` |
| `anstyle` | `1.0.14` | `MIT OR Apache-2.0` |
| `anstyle-parse` | `0.2.7` | `MIT OR Apache-2.0` |
| `anstyle-query` | `1.1.5` | `MIT OR Apache-2.0` |
| `anstyle-wincon` | `3.0.11` | `MIT OR Apache-2.0` |
| `anyhow` | `1.0.102` | `MIT OR Apache-2.0` |
| `ao2-adapters` | `0.4.75` | `Apache-2.0` |
| `ao2-artifacts` | `0.4.75` | `Apache-2.0` |
| `ao2-cli` | `0.4.75` | `Apache-2.0` |
| `ao2-core` | `0.4.75` | `Apache-2.0` |
| `ao2-policy` | `0.4.75` | `Apache-2.0` |
| `ao2-runtime` | `0.4.75` | `Apache-2.0` |
| `autocfg` | `1.5.0` | `Apache-2.0 OR MIT` |
| `base64ct` | `1.7.3` | `Apache-2.0 OR MIT` |
| `bitflags` | `2.11.1` | `MIT OR Apache-2.0` |
| `block-buffer` | `0.10.4` | `MIT OR Apache-2.0` |
| `bumpalo` | `3.20.2` | `MIT OR Apache-2.0` |
| `cc` | `1.2.62` | `MIT OR Apache-2.0` |
| `cfg-if` | `1.0.4` | `MIT OR Apache-2.0` |
| `chrono` | `0.4.44` | `MIT OR Apache-2.0` |
| `clap` | `4.5.53` | `MIT OR Apache-2.0` |
| `clap_builder` | `4.5.53` | `MIT OR Apache-2.0` |
| `clap_derive` | `4.5.49` | `MIT OR Apache-2.0` |
| `clap_lex` | `0.7.7` | `MIT OR Apache-2.0` |
| `colorchoice` | `1.0.5` | `MIT OR Apache-2.0` |
| `core-foundation-sys` | `0.8.7` | `MIT OR Apache-2.0` |
| `cpufeatures` | `0.2.37` | `MIT OR Apache-2.0` |
| `crc32fast` | `1.5.0` | `MIT OR Apache-2.0` |
| `crypto-common` | `0.1.7` | `MIT OR Apache-2.0` |
| `digest` | `0.10.7` | `MIT OR Apache-2.0` |
| `equivalent` | `1.0.2` | `Apache-2.0 OR MIT` |
| `errno` | `0.3.14` | `MIT OR Apache-2.0` |
| `fastrand` | `2.4.1` | `Apache-2.0 OR MIT` |
| `filetime` | `0.2.39` | `MIT/Apache-2.0` |
| `find-msvc-tools` | `0.1.9` | `MIT OR Apache-2.0` |
| `flate2` | `1.1.9` | `MIT OR Apache-2.0` |
| `futures-core` | `0.3.32` | `MIT OR Apache-2.0` |
| `futures-task` | `0.3.32` | `MIT OR Apache-2.0` |
| `futures-util` | `0.3.32` | `MIT OR Apache-2.0` |
| `generic-array` | `0.14.7` | `MIT` |
| `getrandom` | `0.2.17` | `MIT OR Apache-2.0` |
| `getrandom` | `0.3.3` | `MIT OR Apache-2.0` |
| `hashbrown` | `0.16.1` | `MIT OR Apache-2.0` |
| `heck` | `0.5.0` | `MIT OR Apache-2.0` |
| `iana-time-zone` | `0.1.65` | `MIT OR Apache-2.0` |
| `iana-time-zone-haiku` | `0.1.2` | `MIT OR Apache-2.0` |
| `indexmap` | `2.13.0` | `Apache-2.0 OR MIT` |
| `is_terminal_polyfill` | `1.70.2` | `MIT OR Apache-2.0` |
| `itoa` | `1.0.18` | `MIT OR Apache-2.0` |
| `js-sys` | `0.3.98` | `MIT OR Apache-2.0` |
| `libc` | `0.2.386` | `MIT OR Apache-2.0` |
| `linux-raw-sys` | `0.12.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` |
| `log` | `0.4.29` | `MIT OR Apache-2.0` |
| `memchr` | `2.8.0` | `Unlicense OR MIT` |
| `miniz_oxide` | `0.8.9` | `MIT OR Zlib OR Apache-2.0` |
| `num-traits` | `0.2.39` | `MIT OR Apache-2.0` |
| `once_cell` | `1.21.4` | `MIT OR Apache-2.0` |
| `once_cell_polyfill` | `1.70.2` | `MIT OR Apache-2.0` |
| `pin-project-lite` | `0.2.37` | `Apache-2.0 OR MIT` |
| `proc-macro2` | `1.0.106` | `MIT OR Apache-2.0` |
| `quote` | `1.0.45` | `MIT OR Apache-2.0` |
| `r-efi` | `5.3.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` |
| `rustversion` | `1.0.22` | `MIT OR Apache-2.0` |
| `ryu` | `1.0.23` | `Apache-2.0 OR BSL-1.0` |
| `same-file` | `1.0.6` | `Unlicense/MIT` |
| `serde` | `1.0.228` | `MIT OR Apache-2.0` |
| `serde_core` | `1.0.228` | `MIT OR Apache-2.0` |
| `serde_derive` | `1.0.228` | `MIT OR Apache-2.0` |
| `serde_json` | `1.0.149` | `MIT OR Apache-2.0` |
| `serde_yaml` | `0.9.34+deprecated` | `MIT OR Apache-2.0` |
| `sha2` | `0.10.9` | `MIT OR Apache-2.0` |
| `shlex` | `1.3.0` | `MIT OR Apache-2.0` |
| `simd-adler32` | `0.3.9` | `MIT` |
| `slab` | `0.4.12` | `MIT` |
| `strsim` | `0.11.1` | `MIT` |
| `syn` | `2.0.117` | `MIT OR Apache-2.0` |
| `tar` | `0.4.45` | `MIT OR Apache-2.0` |
| `tempfile` | `3.27.0` | `MIT OR Apache-2.0` |
| `typenum` | `1.20.0` | `MIT OR Apache-2.0` |
| `unicode-ident` | `1.0.24` | `(MIT OR Apache-2.0) AND Unicode-3.0` |
| `unsafe-libyaml` | `0.2.31` | `MIT` |
| `utf8parse` | `0.2.3` | `Apache-2.0 OR MIT` |
| `uuid` | `1.18.1` | `Apache-2.0 OR MIT` |
| `version_check` | `0.9.5` | `MIT/Apache-2.0` |
| `walkdir` | `2.5.0` | `Unlicense/MIT` |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` |
| `wasi` | `0.14.2+wasi-0.2.4` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` |
| `wasm-bindgen` | `0.2.321` | `MIT OR Apache-2.0` |
| `wasm-bindgen-macro` | `0.2.321` | `MIT OR Apache-2.0` |
| `wasm-bindgen-macro-support` | `0.2.321` | `MIT OR Apache-2.0` |
| `wasm-bindgen-shared` | `0.2.321` | `MIT OR Apache-2.0` |
| `winapi-util` | `0.1.11` | `Unlicense OR MIT` |
| `windows-core` | `0.62.2` | `MIT OR Apache-2.0` |
| `windows-implement` | `0.60.2` | `MIT OR Apache-2.0` |
| `windows-interface` | `0.59.3` | `MIT OR Apache-2.0` |
| `windows-link` | `0.2.3` | `MIT OR Apache-2.0` |
| `windows-result` | `0.4.1` | `MIT OR Apache-2.0` |
| `windows-strings` | `0.5.1` | `MIT OR Apache-2.0` |
| `windows-sys` | `0.61.2` | `MIT OR Apache-2.0` |
| `wit-bindgen-rt` | `0.39.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` |
| `xattr` | `1.6.1` | `MIT OR Apache-2.0` |
| `zmij` | `1.0.21` | `Zlib` |
