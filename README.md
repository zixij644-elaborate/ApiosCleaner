# ApiosCleaner

*ἄπιος (ápios) — ancient Greek for "pear"*

A fast cross-platform app cleaner with a portable Rust core. The project started
as a rewrite of [Pearcleaner](https://github.com/alienator88/Pearcleaner), and is
evolving independently through ongoing refactoring and new features.

The engine is platform-agnostic: matching, scanning, and orphan-detection logic
are pure Rust with no OS API dependency. Platform-specific behaviors (file
layout, app metadata, trash semantics) live in a thin adapter layer, so the same
core can ship as per-OS builds, each tuned for its platform.

> ⚠️ **Status**: Core engine PoC. macOS adapter works (CLI); Linux/Windows
> adapters and the GUI are planned.

## Why this project

- **Speed**: full scan in ~0.4–2s
- **Testability**: 32 unit tests covering scan/match/orphan/trash semantics
- **Portable core**: type-checks for non-macOS targets with zero changes
  (e.g. the Mach-O fat parser is fully cross-platform, verified byte-identical to `lipo`)
- **Per-OS builds**: one engine, platform adapters that can be optimized
  independently for each operating system
- **Future roadmap**: mdfind Spotlight supplement → dev-environment cleanup →
  platform adapters (Linux, Windows) → GUI shell

## Status

| Area | State |
|---|---|
| Core engine (scan/match/orphan/trash) | ✅ implemented + unit tested (32/32) |
| CLI (`list` / `uninstall` / `orphan` / `clean-orphan`) | ✅ works on macOS, output verified against the reference implementation |
| Platform adapters | ⚠️ macOS: paths / app metadata / trash in place; Linux, Windows: ⬜ planned |
| Verification | ✅ 9/9 and 17/17 file sets identical on test apps |
| UI | ⬜ planned |

## Build

```sh
cargo build --release
```

## Usage

The `<app>` argument accepts a full path, an app name (auto-looked-up in the
default application folders), or `.` for the current directory. Deleting
commands ask for confirmation (`y/N`, default no); pass `-y` to skip it
(for scripting or GUI/automation integration).

```sh
# List all related files of an app (read-only)
./target/release/apios list /Applications/SomeApp.app
./target/release/apios list SomeApp

# Uninstall an app: the bundle and ALL related files, moved to Trash
./target/release/apios uninstall SomeApp

# List orphaned files left behind by uninstalled apps (read-only)
./target/release/apios orphan

# Delete all orphaned files
./target/release/apios clean-orphan

# List dev environment cache sizes (read-only)
./target/release/apios dev-clean

# Clean one dev environment (e.g. Cargo, Gradle, Xcode), or "all"
./target/release/apios dev-clean cargo
```

## License

The initial codebase was derived from [Pearcleaner](https://github.com/alienator88/Pearcleaner)
by alienator88, licensed under the **Apache License 2.0 with the Commons Clause
License Condition v1.0**. This project is distributed under the **same license**,
including the prohibition on selling the Software. See [LICENSE.md](LICENSE.md).
