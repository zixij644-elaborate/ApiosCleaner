# ApiosCleaner

*ἄπιος (ápios) — ancient Greek for "pear"*

A fast macOS app cleaner with a portable Rust core. The project started as a
rewrite of [Pearcleaner](https://github.com/alienator88/Pearcleaner), and is
evolving independently through ongoing refactoring and new features.

> ⚠️ **Status**: Core engine PoC. CLI works; GUI not yet implemented.

## Why this project

- **Speed**: full scan in ~0.4–2s
- **Testability**: 32 unit tests covering scan/match/orphan/trash semantics
- **Portable core**: pure-logic engine layers with no macOS API dependency
  (e.g. the Mach-O fat parser is fully cross-platform, verified byte-identical to `lipo`)
- **Future roadmap**: mdfind Spotlight supplement → dev-environment cleanup → Homebrew → UI shell

## Status

| Area | State |
|---|---|
| Core engine (scan/match/orphan/trash) | ✅ implemented + unit tested (32/32) |
| CLI (`list` / `list-orphaned` / `uninstall` / `uninstall-all` / `remove-orphaned`) | ✅ works, output verified against the reference implementation |
| Verification | ✅ 8/8 and 16/16 file sets identical on test apps |
| UI | ⬜ planned |

## Build

```sh
cargo build --release
./target/release/apios-cleaner list /Applications/SomeApp.app
```

## License

This project is a **derivative work** of [Pearcleaner](https://github.com/alienator88/Pearcleaner)
by alienator88, which is licensed under the **Apache License 2.0 with the Commons Clause License Condition v1.0**.

This project is distributed under the **same license**: Apache 2.0 with Commons Clause.
Portions derived from the original project remain subject to that license, including
the prohibition on selling the Software. See [LICENSE.md](LICENSE.md).
