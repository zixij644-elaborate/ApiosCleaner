# ApiosCleaner

*ἄπιος (ápios) — ancient Greek for "pear"*

A fast cross-platform app cleaner with a portable Rust core. The project
started as a rewrite of [Pearcleaner](https://github.com/alienator88/Pearcleaner),
but it is not a straight port: it fixes real safety and correctness defects in
the original, rethinks the architecture for cross-platform use, and is
evolving independently with new features (see the
[Beyond a straight port](CHANGELOG.md#beyond-a-straight-port) section of the
changelog).

The engine is platform-agnostic: matching, scanning, and orphan-detection logic
are pure Rust with no OS API dependency. Platform-specific behaviors (file
layout, app metadata, trash semantics) live in a thin adapter layer, so the same
core can ship as per-OS builds, each tuned for its platform.

> ⚠️ **Status**: v0.1.0 — first release. macOS adapter works (CLI); Linux/Windows
> adapters compile with default XDG behavior; the GUI is planned.

## Why this project

- **Speed**: full scan in ~0.4–2s
- **Testability**: 105 unit tests covering scan/match/orphan/trash/lipo/pkg semantics
- **Portable core**: type-checks for non-macOS targets with zero changes
  (e.g. the Mach-O fat parser is fully cross-platform, verified byte-identical to `lipo`)
- **Per-OS builds**: one engine, platform adapters that can be optimized
  independently for each operating system
- **Safety first**: all deletions are trash-based (reversible), critical system
  paths are protected against normalization tricks, and every destructive
  command asks for confirmation
- **Not a straight port**: real safety and correctness defects are fixed —
  details in the [changelog](CHANGELOG.md#beyond-a-straight-port)
- **Future roadmap**: GUI shell → per-platform adapter polish (Linux
  desktop files / flatpak, Windows registry / Recycle Bin)

## Status

| Area | State |
|---|---|
| Core engine (scan/match/orphan/trash) | ✅ implemented + unit tested |
| CLI (`list` / `uninstall` / `orphan` / `clean-orphan` / `dev-clean` / `pkg` / `lipo`) | ✅ works on macOS, output verified against the reference implementation |
| Platform adapters | ⚠️ macOS: paths / app metadata / trash in place; Linux, Windows: ⬜ planned |
| Verification | ✅ 9/9 and 17/17 file sets identical on test apps |
| UI | ⬜ planned |

## Install

```sh
# From source (macOS)
git clone git@github.com:Zniece/ApiosCleaner.git
cd ApiosCleaner
cargo build --release
# binary at ./target/release/apios
```

Or install the CLI directly with cargo:

```sh
cargo install --git git@github.com:Zniece/ApiosCleaner.git --locked
```

> ⚠️ Deleting commands move files to the Trash and ask for confirmation; they
> never permanently delete. Run the binary without `sudo` — the critical-path
> guard assumes a non-root user.

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

# Package manager category (Homebrew on macOS): list installed packages
./target/release/apios pkg brew list

# Uninstall one package (type auto-detected; warns about dependents first;
# --zap additionally removes cask user config, with extra confirmation)
./target/release/apios pkg brew uninstall git
./target/release/apios pkg brew uninstall --zap firefox

# Remove orphaned dependencies (dry-run is shown before confirmation)
./target/release/apios pkg brew autoremove

# Lipo: scan all apps for universal (fat) binaries and show how much can be
# freed (read-only); or scan a single app
./target/release/apios lipo
./target/release/apios lipo Firefox

# Thin an app's universal binaries to the current architecture (irreversible;
# asks for confirmation). Code signatures are invalidated by default; pass
# --sign to re-sign thinned binaries ad-hoc (codesign -s -)
./target/release/apios lipo thin Firefox
./target/release/apios lipo thin --sign Firefox
```

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — the portable-core + adapter
  pattern, module map, and safety model
- [CHANGELOG.md](CHANGELOG.md) — release history

## License

The initial codebase was derived from [Pearcleaner](https://github.com/alienator88/Pearcleaner)
by alienator88, licensed under the **Apache License 2.0 with the Commons Clause
License Condition v1.0**. This project is distributed under the **same license**,
including the prohibition on selling the Software. See [LICENSE.md](LICENSE.md).
