# CX Passthrough Features

Passthrough is the unsupported-command execution and measurement path. It is not
official command support.

The user-facing shape is:

```sh
cx -- <command...>
```

When the command is a clear official CX shape, auto mode routes it to the
official wrapper. When it is unsupported, passthrough may run it directly if the
local setting is enabled.

Passthrough does not mask argument conflicts or malformed invocations of
CX-owned command roots. Invalid `read`, `insights`, `report`, and `sh` shapes
return the CX parser error even when unsupported passthrough is enabled.
Unsupported native command trees and deliberately parser-risky native flags
remain eligible for passthrough.

## Enablement

Unsupported passthrough is disabled by default.

Enable it in the insights settings table:

```sh
cx insights settings --set passthrough_unsupported_commands=true
```

When explicit auto mode sees an eligible unsupported command while passthrough
is disabled, it returns the same setting command as actionable guidance instead
of only reporting an unknown CX subcommand.

In this environment, operator guidance may enable it along with invocation
recording:

```sh
cx insights settings --set record_invocations=true --set passthrough_unsupported_commands=true
```

Environment support also exists for explicit passthrough enablement.

The explicit environment override remains available when insights recording is
disabled:

```sh
CX_DISABLE_INSIGHTS=1 CX_ENABLE_UNSUPPORTED_PASSTHROUGH=1 \
  cx -- sqlite3 -readonly ~/.cx/db.sqlite 'select 1;'
```

This separates execution policy from telemetry policy. CX executes the
unsupported command without creating or writing an insights database. Without
the explicit passthrough override, disabling insights does not enable
unsupported commands.

## Recursive CX Refusal

Passthrough refuses recursive CX invocation:

```sh
cx -- cx ...
```

This prevents accidental nested CX execution and confusing telemetry.

## Opportunity Metrics

When passthrough output is large enough to matter and small enough to measure
practically, CX records a compression opportunity. The opportunity estimator can
project generic head/tail savings and generated one-line savings for JSON, SSE
JSON, minified HTML/XML, and blob-like output under 1 MB. The direct passthrough
response remains unchanged; the projected output is telemetry for deciding
whether a future official wrapper is worthwhile.

This helps answer:

- which unsupported commands are frequent
- which unsupported commands produce large output
- which command families might deserve official support
- whether a future wrapper could save meaningful tokens or lines

## Official Support Boundary

Passthrough does not mean CX understands the command. It means CX ran the argv
directly and measured the result.

Promoting a passthrough command to official support should require:

- parser coverage
- fake-binary command tests
- output-metric tests where compaction matters
- failure artifact behavior
- installed-binary smoke coverage after release build/install

## Output Contract

Passthrough preserves:

- real process execution
- real exit code
- direct stdout/stderr without applying the opportunity projection
- opportunity metrics when settings permit recording
- native failure output in `~/.cx/cache/failures/passthrough`

On a nonzero exit with captured output, CX appends the standard
`[full output: ...]` recovery pointer to stdout. The artifact stores the native
stdout and stderr before CX adds shell hints. When both native streams are
exactly empty, CX preserves the empty streams and nonzero exit without creating
a zero-byte artifact or pointer. Artifact creation is best effort and never
replaces the native exit code. The cache keeps the same bounded latest-artifact
policy used by official wrappers.

It does not promise:

- semantic compaction
- command-specific parsing
- command repair
- correctness hints beyond generic execution/failure behavior

## Insights Labels

When insights recording is enabled, passthrough invocations use labels such as:

- process: the normalized executable root, such as `jq` or
  `sample-suite-tests`
- command family: a recognized stable family such as `diff` or `find`, or
  `passthrough <program>` for unsupported roots
- observation source: `passthrough:<program>`

Command shape uses the same telemetry-safe program basename, so equivalent
executables invoked from different worktrees group together without retaining
the executable path. Historical rows from older binaries may still use
`passthrough`, `unknown`, or path-specific shapes.

Opportunity records are separate from normal command invocation rows.

Actionable failures also write a `command_failures` row when failure-response
recording is enabled. That row contains the redacted command, bounded CX
response, native response, native source label, and exit code. For nonempty
failures the CX response contains the artifact pointer. For exactly empty
failures both response fields and the artifact reference remain empty while the
exit code and command identity are still recorded.

Commands rejected before passthrough execution are also separate. With
invocation recording enabled, `command_routing_decisions` records whether the
shape was eligible, whether passthrough was disabled, and which parser error
ended routing. Use `cx insights routing --limit 20` to distinguish missing local
passthrough setup from malformed CX-owned commands. These rows do not contribute
to executed invocation or savings totals.

## Command Selection Guide

Use official CX command families when available.

Use `cx -- <command...>` for unsupported commands in environments where
passthrough is enabled.

Use `cx sh` or `cx -- bash -lc` when shell syntax is required.

If passthrough is disabled and the command is needed for the task, enable the
setting when allowed or run the native command with narrow output.
