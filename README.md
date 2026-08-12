[English](README.md) | [中文](README.zh-CN.md)

# ApiosCleaner

*ἄπιος (ápios) — ancient Greek for "pear"*

A fast cross-platform app cleaner with a portable Rust core. It started as a
rewrite of [Pearcleaner](https://github.com/alienator88/Pearcleaner) but is
not a straight port: real safety and correctness defects are fixed, the
architecture is rethought for cross-platform use, and it evolves
independently — see [Beyond a straight port](CHANGELOG.md#beyond-a-straight-port).

> ⚠️ **Status**: v0.2.0 — macOS and Windows adapters work (CLI); Linux
> compiles with default XDG behavior; the GUI is planned.

## Why this project

- **Universal binary thinning (`lipo`)** — a signature Pearcleaner feature
  most other app cleaners lack. Apple Silicon Macs run universal binaries
  (arm64 + x86_64), so most apps carry a dead second architecture; `apios
  lipo` scans apps and thins them, often freeing half the binary's size.
  Byte-identical to Apple's `lipo`, best-slice selection (arm64e, x86_64h
  gated on AVX2), atomic replacement, confirmation-gated
- **Speed**: full scan in ~0.4–2s
- **Portable core, per-OS builds**: matching, scanning, and orphan detection
  are pure Rust with no OS API dependency and type-check unchanged on
  non-macOS targets; platform behavior (paths, trash, Spotlight, package
  managers) lives behind per-OS adapters that can be tuned independently
- **Testability**: 110+ unit tests covering scan/match/orphan/trash/lipo/pkg/
  plugin semantics plus Windows-only suites (registry enumeration, Recycle Bin
  FFI, winget parsing) that run natively on the Windows CI job
- **Safety first**: all deletions are trash-based (reversible), critical
  system paths are protected against normalization tricks, and every
  destructive command asks for confirmation

## Status

| Area | State |
|---|---|
| Core engine (scan/match/orphan/trash) | ✅ implemented + unit tested |
| CLI (`list` / `uninstall` / `orphan` / `clean-orphan` / `dev-clean` / `pkg` / `plugins` / `lipo`) | ✅ works on macOS, output verified against the reference implementation |
| Platform adapters | ✅ macOS: paths / metadata / trash / Spotlight / lipo; ✅ Windows (v0.2.0): registry + Start Menu discovery, Recycle Bin (system API), taskkill, dev-clean, winget; ⬜ Linux: XDG defaults, desktop-file parsing planned |
| Verification | ✅ 9/9 and 17/17 file sets identical on test apps; ✅ Windows native tests on CI (registry / Recycle Bin / winget parsing) |
| UI | ⬜ planned |

## Install

**Pre-built binary** (recommended) — download the latest release from
[GitHub Releases](https://github.com/Zniece/ApiosCleaner/releases/latest),
then unzip and add it to your PATH:

```sh
unzip apios-v0.2.0-macos-universal.zip -d ~/bin
# universal binary: Apple Silicon (arm64) and Intel (x86_64)
```

> ⚠️ macOS Gatekeeper: the binary is ad-hoc signed; the first run from a
> downloaded zip may need right-click → **Open**, or
> `xattr -d com.apple.quarantine ~/bin/apios`.

Or build from source (macOS):

```sh
git clone git@github.com:Zniece/ApiosCleaner.git
cd ApiosCleaner
cargo build --release
# binary at ./target/release/apios
```

Or install the CLI directly with cargo:

```sh
cargo install --git git@github.com:Zniece/ApiosCleaner.git --locked
```

**Windows**: download `apios-windows-x86_64` (a zip with `apios.exe`) from the
latest [release](https://github.com/Zniece/ApiosCleaner/releases/latest) — or
the latest [CI run](https://github.com/Zniece/ApiosCleaner/actions) →
Artifacts for a fresh build — or `cargo install` on the machine itself.
Requires no admin rights; the Recycle Bin API works per-user.

> ⚠️ Deleting commands move files to the Trash (macOS/Linux) or the Recycle Bin
> (Windows) and ask for confirmation; they never permanently delete. Run the
> binary without `sudo` — the critical-path guard assumes a non-root user.

## Usage

All examples assume `apios` is on your PATH (Install above). The `<app>`
argument accepts a full path, an app name (auto-looked-up in the default
application folders), or `.` for the current directory. Deleting commands
ask for confirmation (`y/N`, default no); pass `-y` to skip it (for
scripting or GUI/automation integration).

```sh
# List all related files of an app (read-only)
apios list /Applications/SomeApp.app
apios list SomeApp

# Uninstall an app: the bundle and ALL related files, moved to Trash
apios uninstall SomeApp

# List orphaned files left behind by uninstalled apps (read-only)
apios orphan

# Delete all orphaned files
apios clean-orphan

# List dev environment cache sizes (read-only)
apios dev-clean

# Clean one dev environment (e.g. Cargo, Gradle, Xcode), or "all"
apios dev-clean cargo

# Package manager category (Homebrew on macOS): list installed packages
apios pkg brew list

# Uninstall one package (type auto-detected; warns about dependents first;
# --zap additionally removes cask user config, with extra confirmation)
apios pkg brew uninstall git
apios pkg brew uninstall --zap firefox

# Remove orphaned dependencies (dry-run is shown before confirmation)
apios pkg brew autoremove

# List plugin directories (audio components, preference panes, QuickLook
# generators, screen savers, ... — 18 categories, read-only)
apios plugins

# Show one category (case-insensitive)
apios plugins audio

# Delete plugins, moved to Trash (asks for confirmation; pass a category to
# limit the scope, e.g. `apios plugins --clean audio`)
apios plugins --clean

# Lipo (macOS only): scan all apps for universal (fat) binaries and show how
# much can be freed (read-only); or scan a single app
apios lipo
apios lipo Firefox

# Thin an app's universal binaries to the current architecture (irreversible;
# asks for confirmation). Code signatures are invalidated by default; pass
# --sign to re-sign thinned binaries ad-hoc (codesign -s -)
apios lipo thin Firefox
apios lipo thin --sign Firefox
```

### Windows notes

The `<app>` argument accepts a registry `DisplayName` (e.g. `7-Zip`), the
installer path, or a `.lnk` path — `bundle_identifier` does not exist on
Windows, so matching falls back to display-name / path needles.

```sh
# list / uninstall / orphan work as on macOS (deletion goes to the Recycle Bin)

# Package manager category: winget (no formula/cask distinction)
apios pkg winget list
apios pkg winget uninstall 7-Zip
```

`apios lipo` is macOS-only; on Windows the command is not compiled in. `pkg`
and `plugins` report no managers/categories (nothing to enumerate). The console
is switched to UTF-8 (code page 65001) at startup so Chinese output renders
correctly in legacy cmd/PowerShell consoles.

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — the portable-core + adapter
  pattern, module map, and safety model
- [CHANGELOG.md](CHANGELOG.md) — release history

## License

The initial codebase was derived from [Pearcleaner](https://github.com/alienator88/Pearcleaner)
by alienator88, licensed under the **Apache License 2.0 with the Commons Clause
License Condition v1.0**. This project is distributed under the **same
license**, including the prohibition on selling the Software. The full license
text (Commons Clause condition and Apache License 2.0) is in [LICENSE](LICENSE).
