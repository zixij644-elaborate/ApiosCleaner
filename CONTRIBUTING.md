[English](CONTRIBUTING.md) | [中文](docs/zh-CN/CONTRIBUTING.zh-CN.md)

# Contributing to ApiosCleaner

Thanks for considering a contribution! This project is a cross-platform app
cleaner with a portable Rust core and a per-OS adapter layer. Please read
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) first to understand the layering
— pure logic must never depend on OS APIs.

## Getting started

```sh
git clone git@github.com:zixij644-elaborate/ApiosCleaner.git
cd ApiosCleaner
cargo build
cargo test --all
```

## Quality gates

Before submitting, make sure the local gates pass — CI enforces them:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo check --workspace --target x86_64-unknown-linux-gnu   # Linux cross-check
```

The Linux cross-check is a hard gate: the core must type-check on non-macOS
targets with zero changes. If your change needs a platform-specific behavior,
put it behind a trait in the platform adapter layer, not in the core.

## Rules of the codebase

- **Portable core**: no OS API calls outside `crates/apios-core/src/platform/`.
- **Safety first**: deleting paths go through `trash.rs::validate_path`
  (lexically normalized) and the confirmation flow (`y/N`, default no).
- **Don't rewrite, improve**: the codebase is derived from Pearcleaner but is
  not a straight port — fix defects and simplify instead of replicating.
- **Tests**: new matching/scanning/parsing logic needs unit tests with fixture
  bytes/strings and `tempfile` trees, not live system state.

## Pull request process

1. Fork the repository and create a branch (`git checkout -b fix/...`).
2. Make your change, keeping commits small and focused.
3. Run all four quality gates above.
4. Open a pull request against `main`. Describe what changed and why, and
   what you verified. Screenshots or before/after output for behavior changes
   are appreciated.
5. CI runs the same gates; a red check must be addressed before merge.

## Reporting bugs

Open an issue with the template provided. For security vulnerabilities, do
**not** open a public issue — see [SECURITY.md](SECURITY.md).
