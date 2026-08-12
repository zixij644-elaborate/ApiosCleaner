# Changelog

All notable changes to ApiosCleaner are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-08-12

Windows port: the adapter layer grows a first-class Windows implementation
(hand-written FFI, zero new dependencies), discovered via registry + Start
Menu, deleting via the system Recycle Bin.

### Added

- **Windows platform adapter** — `WindowsAdapter` behind the same traits as
  macOS: `SystemPaths` from `USERPROFILE`/`%APPDATA%`/`%LOCALAPPDATA%`/
  `%PROGRAMDATA%` (app paths filtered to existing dirs), empty macOS-only
  metadata, taskkill-based `ProcessControl`, a 13-environment dev-clean table
  (`%LOCALAPPDATA%`/`%APPDATA%`/`%USERPROFILE%` regenerable caches), and
  winget as the package manager.
- **App discovery** — new `AppDiscovery` trait. macOS/Linux delegate to the
  existing `.app` walk (zero behavior change); Windows enumerates HKLM + HKCU
  uninstall entries (`DisplayIcon` file > `InstallLocation` directory, with
  `REG_EXPAND_SZ` expansion) plus Start Menu `.lnk` files. Empty
  `bundle_identifier` turns off bundle-id matching family automatically, so
  name/path needles keep working.
- **Recycle Bin deletion** — `Trash::move_to_trash` action-level method: the
  POSIX archive move lives in core as `move_to_trash_dir` (macOS/Linux
  unchanged), Windows overrides with hand-written `SHFileOperationW` FFI
  (`FO_DELETE` + `FOF_ALLOWUNDO` → system Recycle Bin, silent, no
  confirmation UI; batch call with per-file failure classification).
- **`pkg winget`** — winget wrapper: `list` (Name/Version table parser that
  survives names with single spaces), `uninstall --name --silent`; no
  formula/cask split, no dependents/autoremove (returns empty). Automation
  flags (`--accept-source-agreements --disable-interactivity`) always set.
- **UTF-8 console** — `SetConsoleOutputCP(65001)` at startup so Chinese
  output renders in legacy cmd/PowerShell code pages.
- **CI Windows artifact** — the windows job now builds `--release` on push
  and uploads `apios-windows-x86_64` (apios.exe) as a downloadable artifact.

### Fixed

- **Security (Windows)**: `normalize_absolute` now preserves the drive
  prefix — previously `C:\Windows` normalized to `/Windows` and bypassed the
  critical-path table. `validate_path` gained a Windows critical table
  (`SystemRoot`, `ProgramFiles`, `ProgramFiles(x86)`, `ProgramData`, profile
  root) plus bare drive-root (`X:\`) format detection; `..`-style bypasses
  (e.g. `C:\Windows\..\Windows`) are caught by the same normalization.
- Archive folder names sanitize Windows-illegal characters
  (`\ < > " | ? *`) alongside `/` and `:`.

### Cross-platform

- The engine core stays POSIX-agnostic; Windows-only code lives in
  `platform/windows.rs` + `win_registry.rs` + `win_trash.rs` + `winget.rs`
  behind `cfg(windows)`. Registry integration tests (HKCU temp key), Recycle
  Bin tests (temp file) and path-safety Windows tests run natively on the
  Windows CI job.
- `Trash` gained a default `move_to_trash` impl; only Windows overrides it.

### Quality

- 110 tests on macOS; additional Windows-only suites (registry / Recycle Bin /
  winget parsing) run on the windows-latest job. Clippy clean with
  `-D warnings`; Linux + Windows cross-checks keep the core portable.

## [0.1.0] - 2026-08-12

First public release. A fast cross-platform app cleaner with a portable Rust
core, derived from [Pearcleaner](https://github.com/alienator88/Pearcleaner).
Goes beyond a straight port — see [Beyond a straight port](#beyond-a-straight-port).

### Added

- **`list <app>`** — find every file an app leaves behind: the bundle, related
  paths, container directories, Spotlight supplemental hits, and force-included
  outliers. Read-only.
- **`uninstall <app>`** — kill the running app, then move the bundle and all
  related files into a timestamped archive folder in the Trash (reversible;
  nothing is permanently deleted).
- **`orphan` / `clean-orphan`** — find files left behind by apps that were
  uninstalled manually, then (with confirmation) move them to the Trash.
- **`dev-clean [env]`** — list sizes of dev-environment caches (Cargo, Gradle,
  Xcode, VS Code, etc.) and clean them. Only regenerable caches are listed.
- **`pkg brew list|uninstall|autoremove`** — Homebrew package management:
  dependents are checked before uninstall (with `--zap` support for cask user
  config), and orphaned dependencies can be removed.
- **`lipo [app]` / `lipo thin <app> [--sign]`** — scan apps for universal (fat)
  Mach-O binaries and thin them to the current architecture, optionally
  re-signing ad-hoc. The parser handles fat32 and fat64 and was verified
  byte-identical to Apple's `lipo`.
- **Platform adapter layer** — the engine core is pure Rust with no OS API
  dependency; platform behaviors (paths, app metadata, trash, Spotlight,
  process control, dev-env paths, package managers) live behind traits
  dispatched by `cfg(target_os)`. macOS is fully wired; Linux/Windows adapters
  compile with sane XDG defaults.
- **CI gates** — GitHub Actions workflow running fmt, clippy (`-D warnings`),
  the full test suite, and a Linux cross-compile check of the workspace.

### Beyond a straight port

#### Safety fixes

- **Path validation runs on lexically normalized paths.** `..`, duplicate
  slashes, and trailing slashes cannot bypass the protected-path list
  (`/Library/..`, `//Applications`, `/System/`); relative paths are rejected.
  `/Users`, `/Users/Shared`, and `~/Applications` are protected too.
- **The "Wrapper" ancestor jump fires only when the immediate parent is named
  `Wrapper`** — a real wrapped-app structure. Broader substring matching could
  pull a whole directory into the delete set.
- **Symlinked `.app` bundles are resolved, not skipped**, so live apps never
  appear in the orphan list.
- **The writable check follows POSIX rename semantics**: the parent directory
  must be writable, not the item itself (read-only files are deletable).

#### Correctness fixes

- **Exact bundle-id matching** — VS Code Insiders never triggers the stable
  build's force-include rules.
- **Final dedup compares against all retained paths**, so an ancestor can
  never escape the filter.
- **Cross-volume moves** fall back to copy + remove (`EXDEV`).
- **Homebrew error triage**: "no dependents" (exit 0) is not a failure, and
  cask slow paths don't look like errors.
- **Empty result sets exit 0** — "nothing to do" is not an error.

#### Performance

- **Orphan detection** uses a prebuilt UUID→bundle-id map instead of O(N²)
  directory rescans.
- **The Mach-O fat parser** reads only the header and slice table — never
  whole files — with bounded reads and truncation checks.
- **Directory-size walks** skip symlinks (loop-safe) via `lstat`.

#### Cross-platform architecture

- The engine is pure Rust with zero OS API dependency; every platform behavior
  sits behind a trait dispatched by `cfg(target_os)`. Linux/Windows adapters
  compile today with XDG defaults, and a Linux cross-check gate keeps the core
  portable.
- The fat-binary parser is verified byte-identical to Apple's `lipo`.

#### CLI ergonomics

- Fully scriptable: destructive commands print what they will do, take `-y`,
  and use consistent exit codes (0 success/abort, 1 error) — no interactive
  TUI required.
- Confirmation defaults to no (`y/N`); a rejection reports
  `Aborted — nothing was deleted.` and exits 0.

### Quality

- 105 unit tests covering scan/match/orphan/trash/lipo/pkg semantics.
- Clippy clean with `-D warnings`; Linux cross-check gate keeps the core
  portable.
- File sets verified identical to the reference implementation on test apps
  (9/9 and 17/17).
