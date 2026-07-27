# Changelog

CX follows semantic versions for public releases. Internal `rN` tags remain
engineering revision markers and are not package versions.

## [Unreleased]

### Added

- Checksum-verified one-line installer hosted at `install-cx.asi.sh`.

### Changed

- Public settings documentation now leads with the complete per-setting CLI
  command and separates default values, accepted arguments, and descriptions.

## [0.1.1] - 2026-07-27

### Changed

- Local invocation metrics, redacted command shapes, unsupported passthrough,
  and command optimizations are enabled by default.
- Full command text, source labels, failure responses, and response previews
  remain opt-in.
- Existing explicit settings remain authoritative when upgrading.

## [0.1.0] - 2026-07-27

Initial public release.

### Added

- Public GitHub presentation and repository metadata.
- Transparent local SQLite insights documentation and screenshots.
- Native GitHub release binaries for macOS and Linux on arm64 and x64.
- `@contextlimit/cx` npm distribution with checksum-verified binary install.
- `contextlimit/homebrew-tap` binary formula generation.
- Simple and advanced Codex `~/.codex/AGENTS.md` instructions.
- Contribution, security, conduct, issue, pull-request, and CI surfaces.

### Changed

- Community links now point to the maintainer's current Discord, X, YouTube,
  and Stack Overflow profiles.
- Stale examples now use the public `cx --` auto-mode contract and current
  source paths.

### Removed

- Remote telemetry mirroring. CX insights and failure artifacts are local-only.

### Highlights

- Direct execution of named high-output command families.
- Compact review output with command-specific evidence retention.
- Exact source-range and Git evidence routes.
- File-backed process capture and recoverable failure artifacts.
- Optional local SQLite metrics for savings, expansion, failures, reports,
  repairs, routing decisions, and future wrapper opportunities.
- Explicit unsupported-command passthrough policy.
- Command-quality reporting for wrong or misleading successful output.

[Unreleased]: https://github.com/contextlimit/cx/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/contextlimit/cx/releases/tag/v0.1.1
[0.1.0]: https://github.com/contextlimit/cx/releases/tag/v0.1.0
