# CX Insights Features

CX insights record command savings, failures, reports, rejected routing
decisions, settings, and unsupported passthrough opportunities in SQLite.

The default database path is:

```text
~/.cx/db.sqlite
```

Tests and experiments should use:

```sh
CX_INSIGHTS_DB_PATH=<project>/.tmp/db.sqlite
```

so the real user database is not deleted, migrated, or overwritten.
An isolated database starts from an all-enabled testing baseline, but explicit
rows written with `cx insights settings --set ...` override that baseline.

## Recording Settings

Settings live in the `settings` table. The user-facing command is:

```sh
cx insights settings
cx insights settings --set key=true
cx insights settings --set key=false
```

Important settings:

- `record_invocations`: passively record command invocation metrics.
- `record_command_text`: store readable command text and argv JSON.
- `record_command_shape`: store redacted command shape and stable shape hash.
- `record_sources`: store command output source or target labels.
- `record_failures`: passively record actionable failed command details.
- `record_failure_responses`: store bounded CX and non-CX failure responses.
- `passthrough_unsupported_commands`: direct-exec unsupported command families.
- `command_optimizations`: enable optional CX command optimizations.

Insights recording is disabled by default unless enabled by settings or
environment. Command text recording is optional and controlled separately.

Operators that need an unsupported command without recording can combine
`CX_DISABLE_INSIGHTS=1` with
`CX_ENABLE_UNSUPPORTED_PASSTHROUGH=1`. This is useful for inspecting the CX
database itself without adding an invocation row. Prefer the supported
`cx insights ...` commands for routine audits.

## Metrics

CX records text metrics for raw, emitted, saved, and expanded output:

- bytes
- characters
- lines
- approximate tokens

Saved metrics are the positive part of raw minus emitted values. Expanded metrics
are the positive part of emitted minus raw values. CX records an expansion count,
the largest observed token expansion, and a stable expansion reason such as
`status-summary`, `syntax-check-summary`,
`failure-artifact-recovery-hint`, or `read-formatting`. The net token delta is
emitted tokens minus raw tokens, so a positive value means net expansion and a
negative value means net savings. Compression ratios remain derived from raw and
emitted values.

These metrics support analysis such as:

- total tokens saved
- largest savings by command
- commands with zero savings
- commands that expand output
- failure hotspots
- command families that deserve new wrappers

## Command Identity

Insights separate command identity into useful layers:

- process/root command
- command family
- readable command text when enabled
- argv JSON when enabled
- command shape and shape hash when enabled
- source labels when enabled

This lets analysis group `git`, `git diff`, and individual redacted command
shapes separately.

## Redaction

Command text and argv are redacted before storage. CX strips obvious tokens,
passwords, API keys, secret-like values, and common key prefixes such as `sk-`.
Command shape uses a telemetry-safe executable basename rather than retaining a
machine-specific executable path. Structured tool names remain useful for
grouping, while rejected secret-like program values use a redacted shape.

Redaction is intentionally conservative. Users should still avoid enabling
command text or source recording unless they want that level of local telemetry.

## Reports

`cx report <cx-command...>` records a command-quality issue. It is intended for
incorrect results, not only nonzero exits.

Examples:

```sh
cx report cx grep 'a|b' src
cx report cx -- node app/test.mjs
```

Reports help answer:

- which command families are wrong or surprising
- how often a wrapper needs repair
- whether a passthrough or native comparison should become test coverage

CX normalizes away an outer `cx` and optional `--` before storing the reported
command identity. Evidence selection first requires the same redacted command,
family, shape, working directory, thread, and recent time window. When command
text was intentionally not retained, CX may use a command shape only when there
is exactly one matching invocation. It does not guess between several
same-shape commands and it never reruns the reported command automatically.
Shape-only evidence is never borrowed from an invocation whose argv was
retained, because a different command with the same generic shape is not proof
for the report.

Each report exposes an evidence kind such as:

- `exact-command:invocation-preview`
- `exact-command:failure-detail`
- `exact-command:metadata-only`
- `unique-shape:failure-detail`
- `unique-shape:metadata-only`
- `no-match`
- `legacy` for rows created before evidence provenance was recorded

Actionable failure rows carry an optional exact invocation ID. When general
response previews are disabled but `record_failure_responses` is enabled, a
report may recover the bounded, redacted CX and non-CX responses from that exact
failure row. Successful command responses that were not recorded remain
unavailable rather than being reconstructed or borrowed from another command.

Every report starts in the `open` state. The original row in `command_reports`
is not rewritten when the investigation changes state. CX stores the current
classification in `command_report_dispositions`, keyed one-to-one by report ID.
This keeps the original command, issue kind, note, and timestamp intact while
allowing the actionable queue to change.

Lifecycle states have specific meanings:

- `open`: a CX mismatch or suspicious result still needs investigation.
- `resolved`: CX was changed or the underlying issue was otherwise fixed.
- `native-parity`: CX correctly matched the native command; the surprising
  behavior belongs to the native command or shell semantics.
- `not-reproducible`: the report remains useful history, but the mismatch could
  not be reproduced with current evidence.
- `denied`: CX intentionally removes the report from the active engineering
  queue without claiming a product fix or native parity.

Denied reports carry one structured reason:

- `duplicate`
- `insufficient-evidence`
- `invalid`
- `obsolete`
- `unsupported`
- `low-value`

Duplicate denials require `--related-report-id` so the historical row remains
linked to the canonical report in the same database. Reopening or changing a
denied report to another lifecycle state clears the denial reason and related
report ID.

Classify a report with a required reason and an optional revision:

```sh
cx insights report-update 34 --status resolved \
  --note 'serialized shell scripts are rejected before execution' --revision r110
cx insights report-update 30 --status native-parity \
  --note 'native Bash 3.2 produces the same result'
cx insights report-update 33 --status not-reproducible \
  --note 'current CX and native sqlite3 output match'
cx insights report-update 35 --status denied --reason duplicate \
  --related-report-id 34 --note 'exact duplicate of report 34' --revision r127
```

Reopening uses the same command with `--status open` and a note explaining the
new reproduction. `cx insights reports` shows all history by default. Add
`--status open`, `--status resolved`, `--status native-parity`, or
`--status not-reproducible`, or `--status denied` to select one queue. Text,
JSON, CSV, audit, and dashboard output expose total, open, resolved,
native-parity, not-reproducible, denied, and unknown/other counts
independently.
Resolution notes and revision labels are bounded and passed through CX's
secret/key redaction before SQLite storage.

`cx insights report-triage` is a deterministic conservative queue review. It is
dry-run by default and proposes only:

- evidence-free exact duplicate excess rows;
- bare `sh`, `bash`, `--`, `cx`, or empty commands without actionable evidence;
- reports without actionable evidence whose note exactly matches the generic
  placeholder.

Response-backed, invocation-linked, and artifact-linked reports remain open
for human review. Reports with specific notes also remain open unless they are
exact duplicates of a stronger canonical row. Exact duplicate cleanup may
still consolidate metadata-only rows into the strongest canonical report. The
command never reruns a reported command. Review text or JSON output, then apply
the same policy transactionally:

```sh
cx insights report-triage
cx insights report-triage --format json --limit 25
cx insights report-triage --apply
```

Apply is idempotent. A second dry run or apply returns zero proposals unless new
open reports were recorded.

## Insights Commands

The insights CLI includes:

```sh
cx insights summary
cx insights top
cx insights largest
cx insights recent
cx insights daily
cx insights expansions --limit 20
cx insights presentation
cx insights report
cx insights reports
cx insights reports --status open
cx insights report-update <id> --status <state> [--reason <reason>] \
  [--related-report-id <id>] --note <explanation> [--revision <revision>]
cx insights report-triage [--format text|json] [--limit N] [--apply]
cx insights dashboard
cx insights audit
cx insights settings
cx insights impact
cx insights recommend
cx insights opportunities
cx insights routing --limit 20
cx insights archive-summary --archive <sqlite>
cx insights failures
cx insights export
```

Commands support filters such as `--root`, `--command`, `--level`, `--sort`,
`--format`, and `--limit` depending on the subcommand.

## Archive Summaries

`cx insights archive-summary` analyzes one or more archive SQLite files. It is
used for cross-project or cross-machine analysis.

The archive summary distinguishes official wrappers, passthrough behavior,
zero-savings opportunities, expansion, failures, and command-capture quality.

## Output Contract

Insights commands may reduce:

- large telemetry tables
- long command lists
- detailed row data

They must preserve:

- metric definitions
- command grouping meaning
- settings values
- enough row examples to investigate surprising aggregates
- filters used for the report

## Command Selection Guide

Use `cx insights summary` for a quick savings overview.

Use `cx insights recent --limit N` to inspect recent rows.

Use `cx insights expansions --limit N` to inspect only rows where emitted output
was larger than raw output. Each row includes the positive expansion metrics and
its wrapper reason. Add `--root` or `--command` to narrow the analysis.

Use `cx insights failures --limit N` for failed-command analysis.

Failure coverage separates historical capture evidence from current cache
retention:

- `unknown_failure_invocations`: failed invocations without a linked
  `command_failures` detail row
- `silent_failure_rows`: linked details whose CX and native responses are both
  empty, so no artifact is required
- `artifact_linked_failure_rows`: details that stored an artifact reference at
  capture time
- `output_without_artifact_rows`: output-bearing details with no artifact
  reference; these are the actual artifact-coverage gaps
- `families_with_retained_artifacts`: command families whose mapped artifact
  directory currently contains a retained file
- `families_with_linked_but_pruned_artifacts`: families with artifact-linked
  history but no currently retained file

Audit, export, presentation, recommendations, and dashboard projections use
this shared model. Their aggregate coverage totals describe the complete
filtered dataset and do not change with `--limit`; the limit bounds only
returned row collections. A missing detail row is reported as unknown rather
than as a lost artifact, and a silent failure is never counted as an artifact
risk.

Use `cx insights opportunities --limit N` to prioritize future wrappers.

Use `cx insights routing --limit N` to inspect commands rejected before process
execution. Routing telemetry separates CX-owned parser errors from official
wrapper parse errors and unsupported commands rejected because passthrough was
disabled. Add `--root` or `--command` to filter the grouped totals and recent
rows.

Use `cx insights settings` before changing telemetry behavior.

## Presentation Metrics

`cx insights presentation` includes a metric scorecard generated from the same
authoritative aggregate totals used by the JSON and CSV exports. It reports raw,
emitted, saved, and expanded bytes, characters, lines, and estimated tokens;
average saved tokens per invocation; net token delta; the character savings
ratio; failure and expansion rates; the largest observed token expansion; and
equivalent 200,000-token context windows saved. It also reports nearest-rank
saved-token percentiles for all invocations and for savings-positive
invocations, plus largest/top-10 concentration and totals with those outliers
removed. These values add context to the exact totals; they do not cap, rewrite,
or discard any recorded invocation.

The machine-readable form is:

```sh
cx insights export --format json --limit 25
```

Its `presentation.metrics` object contains numeric values rather than formatted
prose. Ratios are fractions from `0.0` to `1.0`, and
`context_windows_saved` may be fractional. CSV exports carry the same values in
the `presentation_metrics` and `savings_distribution` sections. Export schema
version 18 also exposes top-level `savings_distribution` with:

- invocation and savings-positive invocation counts
- p50, p95, and p99 saved tokens across all matching invocations
- p50, p95, and p99 across only invocations that saved tokens
- largest and top-10 saved-token totals and shares
- saved-token totals excluding the largest invocation and top 10 invocations

Distribution calculations use the complete filtered dataset and are independent
of `--limit`, which only bounds returned row collections. The dashboard's
`source_export_schema_version` comes from the export implementation directly so
the two contracts cannot silently advertise different versions.

## Dashboard UI Contract

`cx insights dashboard --limit N` is the preferred bounded UI snapshot. It is
an on-demand CLI response, not a streaming endpoint. Every row collection is
bounded by the applied limit, while aggregate totals continue to describe all
matching invocations.

```sh
cx insights dashboard --limit 25
```

Dashboard schema version 12 includes:

- `contract`: metric, ratio, timestamp, filter, and refresh semantics
- `provenance`: database presence, source tables, and failure-artifact location
- `settings`: keyed values plus descriptions and sensitivity classes
- `capabilities`: recording/privacy state and available UI sections
- `empty_state`: distinct missing-database, disabled-recording, no-data, and
  privacy-preserving metric states
- `summary`, `cards`, and `charts`: aggregate and presentation projections
- `savings_distribution`: nearest-rank percentiles and concentration context
  over every matching invocation, including robust totals outside the top saves
- `tables`: root/family totals, recent and largest invocations, expansion
  drilldowns, quality reports, artifact coverage, passthrough opportunities,
  and rejected routing decisions
- `recommendations` and `presentation`: evidence-backed actions and numeric
  presentation metrics derived from the canonical export evidence loader
- `health`: failures, reports, response-detail coverage, output-bearing
  artifact gaps, retained versus linked-but-pruned artifacts, expansions,
  routing-rejection totals, net token delta, and estimated future opportunity

Invocation rows now carry `raw`, `emitted`, `saved`, and `expanded` metric
objects together. This lets a UI explain both realized savings and the amount
of evidence CX retained without reconstructing values from ratios. Opportunity
rows are explicitly marked with `estimate: true`; they are not included in
realized savings totals.

The dashboard does not create the database when it is missing. A consumer must
honor `empty_state.database_missing` and require an explicit settings action
before enabling recording.

The current SQLite schema version is `19`, the export schema version is `18`,
and the dashboard schema version is `12`. SQLite `command_invocations` rows carry
`expanded_bytes`, `expanded_chars`, `expanded_lines`, `expanded_tokens`, and
`expansion_reason`. `command_totals` carries aggregate expansion count and
metrics plus `best_expanded_tokens`. The current schema also carries routing,
repair, report-disposition, report-evidence provenance, exact
failure-to-invocation linkage, binary-identity, command-shape, and artifact
attribution data. Failure command and response text is redacted before storage.
Migrations are additive where possible; historical expanded
rows that predate explicit reasons retain
`legacy-unclassified-expansion` rather than inventing a cause.

Routing rows are written only when `record_invocations` is enabled. They always
carry bounded analytical identity: process, command family, decision reason,
Clap error kind, explicit-auto state, and passthrough eligibility/enablement.
Readable command text and redacted argv are stored only when
`record_command_text` is enabled. Redacted command shape and its stable hash are
stored only when `record_command_shape` is enabled. A routing rejection is not
an executed invocation and therefore does not increment invocation, failure, or
savings totals.

These metrics describe observed output only. They do not claim that approximate
tokens equal provider billing tokens, and they do not mix unsupported-command
opportunity estimates into realized savings.
