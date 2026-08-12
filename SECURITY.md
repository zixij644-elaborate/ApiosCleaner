# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security vulnerabilities. Report
them privately through GitHub's Security Advisory feature:

1. Go to the repository's **Security** tab.
2. Click **Report a vulnerability**.
3. Describe the issue: the affected command or code path, a minimal
   reproduction (what an attacker would need to trigger it), and the impact.

Reports are acknowledged as soon as possible, and fixes are published as a
security release when ready.

## Scope

This project is an app cleaner: its core deletes and moves files, so bugs in
path handling have real destructive potential. Issues of particular interest:

- Bypass of the critical-path protection (`trash.rs::validate_path`)
  allowing deletion of protected system paths
- Path normalization / traversal issues that let crafted paths escape their
  intended scope
- Confirmation-bypass flows (destructive actions without user consent)
- Orphan detection misclassifying live apps as deletable

## Supported versions

| Version | Supported |
|---|---|
| latest release (v0.1.x) | ✅ |
| earlier releases | ❌ |

Only the latest release receives security fixes.
