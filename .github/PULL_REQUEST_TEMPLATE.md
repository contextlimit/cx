## Scope

Describe the command family, parser shape, insight, packaging surface, or defect
changed by this pull request.

## Native Authority

- Native command:
- Native exit code:
- Evidence that must remain:

## CX Behavior

- Previous CX output:
- New CX output:
- Can output expand? If yes, why and by how much?
- Failure artifact behavior:

## Privacy And Storage

- Does this store new command text, output, paths, source labels, or report data?
- Does redaction run before persistence?
- Were all tests run with an isolated `CX_INSIGHTS_DB_PATH`?

Do not attach a real `~/.cx/db.sqlite` or private failure artifact.

## Validation

- [ ] Focused parser/command tests
- [ ] Fake-binary forwarding tests
- [ ] Output reduction and evidence-retention tests
- [ ] Failure artifact tests when applicable
- [ ] `cargo test`
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -W clippy::too_many_lines -W clippy::cognitive_complexity`
- [ ] Installed-binary smoke when routing, install, or runtime behavior changed
- [ ] Documentation and feature catalog updated
