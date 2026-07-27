# CX Process Inventories

CX provides official support for `ps` because broad process inventories can be
large while most process troubleshooting needs an executable-level overview.
The wrapper preserves exact process probes and exit codes, and compacts only
large inventories that CX can parse confidently.

## Accepted Shapes

The command accepts native `ps` arguments as trailing argv:

cx -- ps -axo pid,ppid,etime,command

Common examples include:

```text
cx -- ps aux
cx -- ps -ef
cx -- ps -p 1234 -o pid=,ppid=,etime=,command=
cx -- ps --pid 1234,5678 -o pid,command
cx -- ps --no-compact -axo pid,ppid,etime,command
```

`--no-compact` is a CX option. CX removes it before invoking the real `ps`
binary. All other arguments are forwarded directly without shell evaluation.

## Routing And Conversion

`cx -- ps ...` and `cx ps ...` both route to the official process command. CX
resolves `ps` through the normal controlled binary lookup, captures stdout and
stderr through the shared file-backed runner, and preserves the direct child
exit code.

CX selects one of three output paths:

1. PID-selected queries are exact. Shapes using `-p`, `--pid`, `-q`,
   `--quick-pid`, attached PID forms, or a bare comma-separated PID list return
   the real stdout unchanged.
2. `--no-compact` is exact regardless of table size.
3. Other successful outputs with at most 80 nonempty lines are exact. Larger
   outputs are summarized only when CX can identify a final command column and
   parse at least 90 percent of process rows.

The structured summary groups rows by executable basename. Each displayed group
contains its process count, up to four representative PIDs when a PID column is
available, and one bounded command example. Groups are ordered by descending
count and then executable name. When more than 25 executables exist, CX emits a
deterministic bounded catalog of the remaining executable names.

Command examples pass through CX secret redaction before display. A process
argument such as `--token=sk-...` is represented as `[REDACTED]`; it is not
copied into the summary or recovery command.

## Unstructured Output

CX does not guess when a custom `ps -o` layout places the command field before
later columns, when a localized header cannot be recognized, or when too many
rows fail parsing. Large unstructured output receives a labeled bounded
head/tail window instead of a fabricated process summary.

Both structured and unstructured compact views end with a recovery command:

```text
[full process table: cx -- ps --no-compact ...]
```

Run that command when exact rows are required for machine input or detailed
inspection.

## Failures

Nonzero exits remain nonzero. PID-selected failures retain exact stdout and real
stderr. Broad failures use the normal bounded stdout fallback while preserving
stderr. CX attempts to store the full captured output under
`~/.cx/cache/failures/ps/` and adds the standard `[full output: ...]` artifact
hint when storage succeeds.

No-process results are therefore not converted into successful summaries. The
calling script or agent can distinguish an empty successful inventory from a
native `ps` failure by exit code.

## Insights Identity

When invocation recording is enabled, official process calls use:

```text
process: ps
command_family: ps
source: ps
```

Raw metrics describe the captured native stdout and stderr. Emitted metrics
describe the exact or summarized CX response. Broad inventories therefore
produce measured saved bytes, characters, lines, and approximate tokens rather
than opportunity estimates from unsupported passthrough.

Command text and argv remain subject to the insights settings and the shared
secret-redaction policy. The summary itself also redacts command examples, so
enabling command-text recording does not weaken the visible-output boundary.

## Output Contract

- Exact PID probes stay exact.
- `--no-compact` always returns the full native table.
- Small outputs stay exact.
- Large parseable inventories are grouped deterministically.
- Large unparseable inventories use a labeled bounded fallback.
- Process command examples are bounded and redacted.
- Native exit codes and stderr are preserved.
- Failures retain artifact recovery when possible.
- Every compact view provides an explicit full-table recovery command.
