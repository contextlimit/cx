# CX Validation Features

CX validation is built around two principles:

- preserve real command truth
- prove output reduction without losing required evidence

The validation surface includes unit tests, integration tests, recent-call
regressions, output-metric tests, install-script tests, and benchmark compile
targets.

## Test Families

Important test files include:

- `tests/documented_commands.rs`
- `tests/recent_calls.rs`
- `tests/output_metrics.rs`
- `tests/output_expansion_metrics.rs`
- `tests/grep_capture.rs`
- `tests/install_script.rs`
- `tests/insights_command_reports.rs`
- `tests/insights_report_triage.rs`
- `tests/repo_hygiene.rs`

Unit tests inside command modules cover parser and formatter behavior close to
the implementation.

## Recent Calls

Recent-call tests preserve real command shapes observed in agent usage. They are
important because natural command shapes can be hard for Clap and wrapper logic:

- grep patterns that look like flags
- exact grep/rg `--no-compact` routing without pattern/path shifts
- multiple Cargo test filters
- `ctest -N -R`
- find predicates in different orders
- filename and path predicate conjunction for bounded find discovery
- modest shell output that stays exact without static pipeline recognition
- Node stdin runtime flags
- Git evidence commands
- CRLF and unterminated exact read-like ranges
- partial `find` results paired with traversal failures

## Output Metrics

Output metric tests compare raw output with filtered output. A passing compaction
test should show both:

- meaningful byte, line, or token reduction
- retained evidence needed to make the correct decision

Savings without evidence retention is not a good CX feature.

Expansion tests cover the other side of the contract. Small truthful summaries
may intentionally add context when the native command emits little or nothing,
but that growth must be bounded and attributable. The dedicated expansion suite
asserts stable reason labels and tiny-output token/line ceilings for CTest, Node
syntax checks, grep no-match output, clean Git status, empty find and ls results,
empty Docker container listings, and numbered reads. Large fixtures for the same
families remain in `tests/output_metrics.rs` and must still prove real savings.
Together these tests prevent both unbounded readability overhead on tiny output
and overfitting a wrapper to tiny fixtures that never demonstrate compaction.

Exact-read tests also cover explicit source ranges larger than the generic
shell cap. They require native/CX byte and line parity for statically clear
single-file `sed -n` source selections, including long C++ initializer entries,
while proving that source `cat`, generated paths, JSON, mixed scripts, arbitrary
producers, and base64/blob controls remain bounded.

Insights coverage fixtures distinguish unknown failures, silent details,
artifact-linked history, retained files, linked-but-pruned evidence, and
output-bearing details without artifact references. Audit tests run the same
fixture with different row limits and require identical aggregate coverage
objects.

Report-lifecycle fixtures cover schema 18 to 19 migration, preservation of
existing dispositions, denied reason validation, duplicate linkage, reopen
clearing, response-backed protection, specific-note protection, deterministic
canonical selection, dry-run/apply parity, and second-run idempotence.

## Fake Binaries

Many tests use fake binaries and scoped PATH/HOME guards. This is intentional.
Tests should prove command forwarding and filtering without depending on the
user's live Git repo, Docker daemon, pytest plugins, TypeScript installation, or
cluster state.

## Runner Hygiene

Repo hygiene tests enforce safety rules such as:

- no tracked `.DS_Store`
- naming consistency
- source file-size guidelines
- exact long structured source literals with generated-blob negative controls
- no production raw `Command::output()`

The runner's file-backed capture behavior is part of the product safety model.

## Benchmarks

Criterion hot paths live under:

```text
benches/hot_paths/
```

The thin entrypoint is:

```text
benches/cx_hot_paths.rs
```

The Linux-gated IAI compile target is:

```text
benches/cx_iai_hot_paths.rs
```

Benchmarks are not only elapsed-time checks. They pair hot-path measurement with
output-reduction and evidence-retention assertions.

## Release Proof

For behavior changes, source tests are not enough. The intended release proof is:

```sh
cx -- cargo test
cx -- cargo fmt --check
cx -- cargo clippy --all-targets -- -W clippy::too_many_lines -W clippy::cognitive_complexity
cx -- cargo bench --bench cx_hot_paths --no-run
cx -- cargo bench --bench cx_iai_hot_paths --no-run
cx -- cargo build --release --bin cx
cx -- ./scripts/install.sh
```

Then run installed-binary smokes for the changed command family.

## Documentation Validation

Representative commands in each feature page are executable parser contracts in
`tests/documented_commands.rs`. The test also requires every feature Markdown
page to be linked from `docs/features/index.md`. A documented command shape
should either:

- parse as official CX support
- be clearly labeled as passthrough
- be clearly labeled as conceptual conversion

`tests/install_script.rs` installs the test binary behind the real wrapper,
enables recording only in a temporary `CX_INSIGHTS_DB_PATH`, records a real read
saving, and verifies both the terminal presentation scorecard and JSON export.
This catches examples that parse in source but fail through the installed
runtime path. Installed coverage also verifies the current export/dashboard
schema versions and exact large source-range behavior without touching the real
user database. Installed report-triage coverage uses an isolated database and
must prove schema 19, denied filtering, reason serialization, dry-run/apply
parity, and zero proposals after a second run.

Insights evidence tests also prove that explicit settings override the
all-enabled isolated-database baseline, reports do not guess between ambiguous
command shapes, and retained failure responses remain redacted while linking
back to the exact invocation.

Passthrough routing tests also prove that the explicit unsupported-command
override can execute with insights disabled without creating
`~/.cx/db.sqlite`.

`tests/failure_artifacts.rs` exercises official wrappers, direct-capture paths,
and unsupported passthrough with fake failing binaries. It verifies real exit
codes, artifact content, the standard recovery pointer, retention counts, and
`cx insights failures` correlation. The installed-wrapper suite separately
proves that a failing unsupported command creates its artifact under the
temporary test HOME.

## Command Selection Guide

Run focused tests first while developing a wrapper.

Run full proof before committing behavior changes.

Use fake binaries for wrapper semantics and installed smokes for release-path
confidence.
