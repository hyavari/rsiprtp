All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] — 2026-05-02

### Removed

- **DTLS-SRTP stub** (`rsiprtp::srtp::dtls`). The module never contained a DTLS handshake — only fingerprint parsing, role enums, and a use-srtp-extension codec. SRTP key exchange is via SDES only. If DTLS-SRTP support arrives later it will be designed against an actual DTLS crate, not retrofitted onto these types.

## [0.2.0] — 2026-05-01

### Added

- SRTP and ICE/STUN/TURN types are now reachable through the published facade
  as `rsiprtp::srtp` and `rsiprtp::ice`.

### Changed

- **Workspace collapsed into a single publishable crate.** The eleven internal
  `rsiprtp-*` crates (core, sip, transaction, dialog, transport, sdp, rtp,
  srtp, ice, media, session) are now modules of the `rsiprtp` crate. Source
  layout is unchanged for end users — `rsiprtp::sip::…`, `rsiprtp::rtp::…`,
  etc. resolve as before.
- **Minimum supported Rust version is now 1.88** (previously 1.75). Required
  by `ropus 0.12` (typed runtime bitrate API used by the BitrateBridge) and
  the `time 0.3.47` transitive via `ezk-g722`. Downstream consumers upgrading
  from 0.1.x will need a newer toolchain.
- Minor clippy / MSRV idiom cleanups under stable rustc (`is_multiple_of`,
  `Duration::abs_diff`, `collapsible-match`).

### Removed

- **`opus` feature flag** — Opus codec is now built in. `ropus` is pure-Rust
  and was already unconditionally enabled by `rsiprtp-session`; the flag had
  no off-state and is gone.
- **`dtls` feature flag** and the optional `openssl` dependency. The
  DTLS-SRTP framing types remain in `rsiprtp::srtp`; the handshake itself is
  not yet implemented, so there was nothing for `openssl` to gate.
- Unused `crossbeam` and `dasp` dependencies.
- Heavyweight baresip / Asterisk integration test fixtures from the published
  tarball (`package.exclude`). The framework stays in the repository for
  local use.

### Fixed

- `RegistrationManager::needs_refresh` no longer panics on Windows hosts
  within roughly twelve minutes of system boot. The check used unchecked
  `Instant` subtraction; it now uses saturating arithmetic.
- `generate_tag()` no longer produces duplicate tags on macOS under load.
  The previous implementation seeded from `SystemTime`, whose resolution on
  macOS is too coarse to distinguish back-to-back calls; it now draws from
  `rand::thread_rng()`.

[Unreleased]: https://github.com/0x4D44/rsiprtp/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/0x4D44/rsiprtp/releases/tag/v0.3.0
[0.2.0]: https://github.com/0x4D44/rsiprtp/releases/tag/v0.2.0
