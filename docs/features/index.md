# CX Feature Catalog

This directory documents the official CX feature surface. Each page should answer
four questions:

- what command shapes CX accepts
- what those shapes convert to internally
- what output contract CX promises
- what insights labels or telemetry the default local recording produces

CX remains a narrow command replacement for high-output local commands. It is not
a shell, daemon, MCP server, or generic memory system. Unsupported passthrough is
enabled by default for exact execution and measurement, but passthrough is not
the same thing as official command support.

## Command Families

- [Git](git.md): status, diff, log, show, evidence-diff, conflict-diff.
- [Cargo](cargo.md): cargo test filtering and multi-filter repair.
- [Read](read.md): explicit file windows and read modes.
- [Read-Like Rewrites](read-like.md): cat, head, tail, sed ranges, and nl.
- [Grep And Rg](grep.md): grep dialects, rg aliasing, file lists, and bounded matches.
- [Find](find.md): bounded file discovery.
- [Ls](ls.md): compact directory inventory.
- [Process Inventories](processes.md): exact PID probes and bounded broad process tables.
- [Pytest](pytest.md): Python test summaries.
- [Go Test](go.md): JSON event summaries.
- [TypeScript](tsc.md): TypeScript compiler diagnostics.
- [Node](node.md): syntax checks and bounded runtime support.
- [CMake And CTest](cmake-ctest.md): C++ build and test wrappers.
- [Containers](containers.md): Docker and Kubernetes summaries.
- [Shell And SSH](shell.md): local shell scripts and remote heredoc guidance.
- [Passthrough](passthrough.md): unsupported-command measurement.
- [Insights](insights.md): SQLite telemetry, settings, reports, and exports.
- [Runner](runner.md): process capture and failure artifact boundary.
- [Parser And Dispatch](parser.md): official support, auto mode, and command identity.
- [Validation](validation.md): tests, metrics, benchmarks, and release proof.

## Official Support Versus Passthrough

Official command families are parsed, routed, tested, and documented by CX. They
may compact output, repair narrow command shapes, or produce structured
summaries.

Passthrough runs unsupported commands directly only when the local setting allows
it. Passthrough exists to keep work moving and to record opportunity telemetry.
It does not mean CX understands that command family.

## Raw Evidence Versus Review Output

Some CX commands intentionally compact output for review. Other commands preserve
raw output because another system needs exact evidence. The most important split
is Git:

- `cx git diff` is review-oriented and compact by default.
- `cx git evidence-diff` is raw patch evidence for machine-consumed review and
  commit rules.

Use the page for each command family to choose the correct mode.

## Insights

With the default settings, CX records command metrics in `~/.cx/db.sqlite` or
in the database selected by `CX_INSIGHTS_DB_PATH`. The key metrics are raw
bytes, characters, lines, approximate tokens, emitted equivalents, saved
values, and compression ratios. Command text, source labels, and failure
details remain separate opt-in settings. Unsupported passthrough and command
optimizations are enabled by default but remain configurable.

The feature pages name the command family and observation source where those
labels are important for analysis.
