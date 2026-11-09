# Changelog

All notable changes to Chatly are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

Chatly has no published releases yet.

## [Unreleased]

### Added

- Password lock (Argon2id) and an encrypted vault (XChaCha20-Poly1305) for Instagram and Facebook sessions.
- Optional Touch ID unlock.
- Auto-lock on idle, screen lock, sleep, minimize and hide, and optionally on app switch.
- Isolated, in-memory WebKit views for each site, with outbound links confirmed and stripped of tracking parameters.
- Work laptop check for device management, proxies and HTTPS inspection.
- Continuous integration, issue forms and a pull request template.
- Unit tests for settings, the vault file, the allowlist, link handling, password rules and the Work laptop check. Tests now also enforce three of CONTRIBUTING.md's security rules: only the local shell can call Rust, every command is declared in all three places, and `content.js` lists the same domains as `META_DOMAINS`.
- `AGENTS.md`, imported by `CLAUDE.md`, with the rules every coding agent follows: add unit tests and run fmt, clippy and tests before every commit and push, never commit to `main`, and add no comments.

### Changed

- `LICENSE` now contains the MIT License, matching the rest of the project. The initial commit's `LICENSE` file held the Apache License 2.0 text.
- The minimum Rust version is 1.90, which the locked Tauri crates require. It was listed as 1.85.
- Rust code is formatted with `cargo fmt` using `src-tauri/rustfmt.toml`.

### Fixed

- `scripts/build-release.sh` now checks the binary inside every built `Chatly.app` for your home-folder path. Before, a build with `--target` checked a missing or stale file and still reported success.
