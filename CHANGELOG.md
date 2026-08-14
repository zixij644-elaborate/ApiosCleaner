# Changelog

All notable changes to ApiosCleaner are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`apios apps`** — list installed apps found by discovery (read-only):
  macOS `.app` bundles, Windows registry uninstall entries + Start Menu
  `.lnk`, Linux `.desktop` files. The help text states the coverage
  explicitly (portable/unregistered apps are not listed). Useful as an
  overview and to pick an app for `list` / `uninstall`.

### Fixed

- **Orphan-scan accuracy: the installed set now includes package
  managers, extension hosts and system components** — previously only
  `.app` bundles were "installed", so active tooling data was reported
  as orphaned (macOS walkthrough: 24 orphans, of which 13 were live
  VSCode-extension / input-method / brew-cask data). Three generic
  sources join the needles, driven by data tables not hardcoded apps:
  - package-manager package names (via the `PackageManagers` trait,
    incl. a prefix derivation for `-sdk`/`-cli` style cask names);
  - extension registries (`~/.vscode/extensions`, `~/.cursor/extensions`):
    each extension's ID tail (`golang.go` → `go`) protects its data dir;
  - a system-component table (input methods: wetype/sogou/…).
  Real-machine result: 24 → 9 orphans, false positives cleared while
  genuine leftovers (Pearcleaner residue, …) are kept. Layer principle
  from BleachBit: regenerable data (Caches/) may be misjudged
  acceptably; non-regenerable (extensions, preferences) is protected.

- **Deletion failures are classified and reported per file** — the previous
  "failed to delete N file(s)" gave no reason. Each failed path now reports
  its cause (not found / permission denied with a sudo hint / in use /
  trash unavailable / other), mirroring BleachBit's error-classification
  approach. Covers uninstall, dev-clean, plugins and clean-orphan; verified
  on a real macOS run where system-protected apps and sandbox containers
  now explain themselves.

## [0.2.1] - 2026-08-14

### Added

- **Linux platform adapter** — a first-class Linux implementation behind
  the same traits as macOS/Windows (previously "compiles only" with XDG
  defaults; verified on a Kali arm64 VM):
  - **App discovery** — scans the XDG application directories for
    `.desktop` entries (new pure-logic `desktop.rs` parser: `[Desktop
    Entry]` Name/Exec/Icon/Type/NoDisplay, quoted-path and `env VAR=…`
    Exec handling); `list` / `uninstall` resolve apps by display name.
  - **XDG Trash** — `Trash::move_to_trash` rewritten to the freedesktop
    trash spec: `files/` + `info/` layout, percent-encoded `Path=` and
    `DeletionDate=` in `.trashinfo`, conflict suffixes (`.1`, `.2`, …),
    info-write failure rolls the file back. Layout/format logic lives in
    core (`trash::xdg`); the platform only supplies the trash root.
    Mount-point trashes (`.Trash-$uid`) are a documented TODO.
  - **`pkg apt`** — the package-manager abstraction grows an apt backend:
    `apt list --installed` (2910 packages parsed on the Kali VM),
    `apt-cache rdepends --installed` for dependents, `remove -y` (config
    kept; no purge), `autoremove --dry-run` / `-y`; non-root failures
    hint at `sudo apios pkg apt …` (no implicit elevation).
  - **dev-clean** — 4 system package-manager caches join the Linux table
    (APT/DNF/pacman/Snapd; root-owned dirs route through the existing
    sudo hint); Linux now lists 22 environments.
  - **ProcessControl** — `pgrep -f` + `kill -TERM` (full-command match
    avoids the 15-char process-name truncation).
  - **CI** — new `linux` job (native fmt/clippy/test on ubuntu, release
    artifact on push); cross-check now covers only the Windows target.
- **Cross-platform core infrastructure** — shared building blocks the
  Linux adapter (and future platforms) rely on:
  - `cmd_util`: external-command runner + freed-space regex parsing;
    homebrew and winget now execute through it (one runner for all
    package managers).
  - `PkgKind::Package` — apt/snap style managers have no formula/cask
    split; `supported_kinds()` lets each manager declare its kinds.
  - `desktop.rs` — freedesktop `.desktop` parser (pure text, unit-tested).
  - shared dev-env table — 10 environments with identical paths on macOS
    and Linux (Cargo, Go Modules, Gradle, …) are defined once in core;
    the macOS 25 / Linux 22 environment sets are unchanged.
- **`--except` path skipping** — `uninstall` and `clean-orphan` accept
  repeatable `--except PATH` (exact match, or everything under a directory,
  `~` expanded) so same-name data that does not belong to the app (a source
  checkout, a shared folder) is kept. Skipped count is reported; verified on
  a real uninstall that pulled in a same-name reference repo.
- **`clean-orphan` interactive selection** — candidates are listed with
  numbers; type `1,3-5` (single numbers and ranges), `a`/`all`, or Enter to
  cancel. Only the selected files are moved, so deliberate leftovers (a
  game's save folder, …) are kept. `-y` preserves the old "delete
  everything" behavior for scripting; without an interactive terminal the
  command refuses unless `-y` is given. Ten new unit tests cover the
  selection parser (ranges, dedup, out-of-range, garbage input).
- **Planned, not yet implemented**: filtering by name/path arguments
  (`apios clean-orphan <name>`) for scripted selective deletion.

### Fixed

- **`clean-orphan` no longer demands sudo for the whole list** —
  `check_protected` bailed the entire command when any orphan was root-owned
  (e.g. `/Library/Application Support/PDInstaller`), so the numbered
  interactive list never appeared. Protected entries are now marked
  `[sudo]` in the list and can be skipped; selecting one reports the failed
  move with a sudo hint. `uninstall` keeps its whole-list sudo check (it
  deletes everything).
- **`orphan` output unified** — the read-only list now uses the same
  numbered format (with `[sudo]` markers) as `clean-orphan`, so the
  numbers correspond 1:1 when choosing what to delete.
- **Orphan-scan trash exclusion** — the `skip_reverse` name substring
  ("trash") is replaced by component-level `conditions::is_in_trash`
  (`.Trash` / `Trash` / `.Trash-` prefix). Previously any directory whose
  name contained "trash" was skipped as a false positive, while Linux XDG
  trash and mount-point trashes only matched by coincidence.
- **Linux first-run trash dirs** — `move_to_trash` now creates `files/`
  and `info/` on first use (a fresh HOME has no trash layout yet; caught on
  the Kali VM walkthrough).
- **Linux CLI app lookup** — `find_app_by_name` looked for `<name>.app`
  on Linux; it now matches against the `.desktop` discovery results
  (caught on the Kali VM walkthrough).

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

### Added

- **In-app help** — every command documents itself (`apios help <cmd>`,
  usage lines and per-command notes), including platform-specific behavior
  on Windows.

### Real-device fixes (Windows)

Bugs found on the first native Windows runs after the port landed:

- **FFI link declarations** — `Reg*W` on `advapi32`, `SHFileOperationW` on
  `shell32` (the MSVC toolchain requires explicit link libraries).
- **Registry discovery hardening** — `WOW6432Node` uninstall views are
  enumerated so 32-bit apps are found; paths are unquoted (`"...\App.exe"`);
  prefix matching prefers the registry main entry over `.lnk` clutter
  (Help/Uninstall shortcuts).
- **`winget list` compatibility** — the parser handles winget v1.29 table
  output (names with spaces, sparse columns); app-name prefix matching.
- **CLI cosmetics** — usage line shows `apios` on Windows (not `apios.exe`);
  the not-found message no longer points at macOS paths.
- **Recycle Bin edge cases** — a wider NUL-terminated buffer for
  `SHFileOperationW`; paths that vanished before the call are not counted as
  moved.
- **Junction filtering** — orphan search skips system junctions
  (Application Data / Documents / Templates) that `symlink_metadata` reports.

### Security (audit P0, verified)

- **`uninstall .` can no longer trash the working directory** — the Windows
  path fallback rejected unless the target looks like an app.
- **Windows critical table is case-insensitive** with trailing-dot /
  trailing-space lexical bypasses covered (registry paths are not
  canonicalized by the OS, and Windows paths are case-insensitive).
- **`fAnyOperationsAborted` is read back** — `SHFileOperationW` returning 0
  can still silently abort (locked/denied files); partial failures no longer
  report as full success.
- **Empty-bundle-id guards** — `contains("")` on a formatted bundle id would
  make family/web-app/full-match conditions always-true; empty id now disables
  those matchers cleanly.
- **Reparse-point double check** — orphan search also filters via
  `is_reparse_point()` so a future std narrowing (SYMLINK-only) cannot leak
  junctions into the delete set.

### Correctness (audit P1 + Windows hardening)

- **`winget` version normalization** — `> 1.2.3` version literals are
  stripped of the `> ` prefix before display.
- **Registry enumeration resilience** — a failed `RegEnumKeyExW` skips the
  index instead of dropping the whole hive; `ERROR_MORE_DATA` grows the
  buffer and retries; the name buffer is reused instead of reallocated.
- **`is_writable` under sudo** — `geteuid() == 0` returns true so protected
  detection works when running as root.
- **Home trailing-slash** — a `HOME` ending in `/` can no longer bypass the
  home-root block (normalized in the platform layer, all three adapters).
- **`.Trash` component match** — `contains(".Trash")` became a
  component-boundary check (`is_in_trash`), so `/Foo.Trash/…` paths are not
  misblocked.
- **`lipo thin` total** — size math uses `saturating_sub` (no panic on
  underflow).
- **`plugins --clean`** — now runs `check_protected` like `uninstall`.
- **`DisplayIcon ",N"` index suffix** — stripped before use.
- **Discovery cache** — `discover_installed_apps` runs once per command
  (find_app_by_name + get_app_info_or_exit shared the enumeration).
- **Windows orphan search covers Program Files (+x86)** — `reverse_paths`
  gained both directories; short-name needles (non-ASCII ≥2 chars, e.g. 微信)
  survive the ASCII-5 threshold; non-TTY confirmation is refused up front;
  `adapter()` is a singleton; needle regexes are merged; a failed archive
  folder reports "couldn't create" instead of "Nothing to delete".
- The critical-path table moved into the `SystemPaths` trait (engine core
  carries zero platform details).

### Windows orphan search (real-device fixes, 81 → 45 orphans)

The orphan list is driven by *names*, so already-installed apps and system
components showed up as candidates after Program Files joined the scan. The
root cause: needles are the app's display name ("7-Zip 24.09 (x64)"), which
never matches the vendor directory ("7-Zip"). Fixed by deriving needles from
the executable path itself:

- **Path-derived needles** — up to 3 ancestor directories + the file stem of
  each discovered executable (`Tencent\Weixin\Weixin.exe` → `weixin` +
  `tencent`, `Code.exe` → `code`), threshold ≥3 chars. The exclude direction
  is always safe: over-matching only causes false negatives.
- **Structural dir names never become needles** — a 21-name table
  (programs/startmenu/programfiles roots, local/roaming/appdata, bin/program,
  microsoft/windows, …) so "programfiles" can't match every Program Files
  path.
- **System/structural dirs are filtered from results** — entry names in a
  15-name AppData list (packages/temp/virtualstore/comms/…), `windows*`
  prefixes, and `{common files, msbuild, reference assemblies, wsl,
  modifiablewindowsapps, internet explorer, application verifier}` are
  skipped. `validate_path` only guards critical *roots*; subdirectories would
  otherwise be deleted by `clean-orphan`.
- **Verified on the real machine**: 81 → 132 (with Program Files) → **45**
  orphans. The 45 remaining are genuine residue candidates (EA/ENE/Patriot/
  Verbatim/WD/Thunder Network…) or information-source gaps (dotnet/VulkanRT
  registry entries have no path).
- Known limits: `Documents\xwechat_files` (WeChat 4.0 data) shares no name
  with Weixin.exe; registry entries without DisplayIcon/InstallLocation cannot
  mark their dirs as installed; display-name variants (NVIDIA vs NVIDIA
  Corporation) have no substring relation.
- 6 new `cfg(windows)` unit tests cover dir/stem/ancestor derivation,
  structural-dir skipping, and the system-dir filter (25 names). macOS paths
  untouched (Windows blocks are cfg-isolated).

### Cross-platform

- The engine core stays POSIX-agnostic; Windows-only code lives in
  `platform/windows.rs` + `win_registry.rs` + `win_trash.rs` + `winget.rs`
  behind `cfg(windows)`. Registry integration tests (HKCU temp key), Recycle
  Bin tests (temp file) and path-safety Windows tests run natively on the
  Windows CI job.
- `Trash` gained a default `move_to_trash` impl; only Windows overrides it.

### Quality

- 110 tests on macOS; additional Windows-only suites (registry / Recycle Bin /
  winget parsing, 6 orphan-needle tests) run on the windows-latest job. Clippy
  clean with `-D warnings`; Linux + Windows cross-checks keep the core
  portable.
- Real-device verification on both platforms: a full macOS command walkthrough
  (list/uninstall/orphan/clean-orphan/dev-clean/pkg/plugins/lipo incl. real
  fat-binary thinning + ad-hoc re-signing) and Windows runs on a Win11 25H2
  machine (registry discovery, Recycle Bin, winget, orphan convergence).

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
