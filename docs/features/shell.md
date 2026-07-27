# CX Shell And SSH Features

CX supports shell execution so scripts, pipes, redirects, heredocs, environment
assignments, aliases, functions, and builtins can stay attached to CX telemetry.

The supported local shell surface is:

```sh
cx sh <args...>
cx sh -lc '<shell command>'
cx sh <<'BASH'
...
BASH
```

Auto mode can also run explicit shells:

```sh
cx -- bash -lc '<shell command>'
```

## Local Shell Scripts

For multi-line local scripts, prefer:

```sh
cx sh <<'BASH'
set -euo pipefail
printf '%s\n' ok
BASH
```

This keeps shell parsing, CX telemetry, and shell diagnostics attached to one
command.

When command optimizations are enabled, shell output is bounded by head/tail
windows. Failures store artifacts and append shell-specific hints when available.
Successful stdout at or below 110 lines and 64 KiB remains exact unless
generated-line detection identifies blob-like content. This protects modest
bounded pipelines whose final shell stage is too complex for static analysis,
while keeping large or generated output eligible for reduction.

Statically clear, single-file `sed -n` print ranges over recognized
human-authored source remain exact even when the requested range exceeds the
generic 1,000-line shell cap:

```sh
cx -- bash -lc "sed -n '1,260p' first.cpp; sed -n '1,2500p' second.cpp"
```

This exception applies only when every shell statement is an explicit source
range read. Unbounded source `cat`, generated/codegen paths, ordinary JSON,
multi-file reads, substitutions, mixed scripts, loops, and arbitrary producer
pipelines remain eligible for compaction. `--no-compact` remains the explicit
escape hatch for other exact shell output.

A single bounded `dd` slice of a recognized document, diff, or patch also stays
exact when CX can prove the estimated stdout is at most 1 MiB:

```sh
cx -- bash -lc 'dd if=.tmp/commit.diff bs=1 skip=0 count=49152 2>/dev/null'
```

CX does not apply this exemption when `of=` is present, the size cannot be
bounded, the input is not an exact-read format, or the estimated output exceeds
1 MiB.

## Shell Syntax Through Auto Mode

Use `cx -- bash -lc` when the command needs shell syntax but should be expressed
as an explicit external command:

```sh
cx -- bash -lc 'git status --short | wc -l'
```

Do not expect `cx -- <program>` to interpret pipes, redirects, aliases, or
environment assignments unless a shell is explicitly part of the argv.

## Remote SSH Scripts

For remote multi-line scripts, use:

```sh
cx -- ssh <host> "bash -s" <<'REMOTE'
set -euo pipefail
printf '%s\n' ok
REMOTE
```

This keeps local CX telemetry and stable remote shell parsing.

Avoid zsh-sensitive marker forms such as:

```sh
echo ===$path===
```

Prefer:

```sh
printf "===%s===\n" "$path"
```

## Heredoc Safety

CX has a narrow SSH heredoc helper for safe Python stdin rewrites. It does not
rewrite arbitrary shell heredocs or SQL scripts.

If a shell quoting shape is unsafe or ambiguous, CX should reject it with
guidance rather than silently changing script text.

## Output Contract

Shell support may reduce:

- long stdout/stderr streams
- repeated shell output
- high-output script results
- huge one-line JSON, SSE JSON, minified HTML/XML, and blob-like payloads

Generated one-line payloads are bounded with an explicit marker that includes
the original character count and retains evidence from both ends of the line.
Human-authored source, JSX, CSS, regex, and ordinary prose are not treated as
generated output. Setting `command_optimizations=false` disables both line
windowing and generated-line projection.

It must preserve:

- real shell exit code
- stderr needed to diagnose failures
- full-output artifact hints on failures
- script text semantics when CX is not performing a tested safe rewrite
- every line explicitly selected by an authoritative human-source `sed -n`
  range

## Insights Labels

When insights recording is enabled, shell invocations are grouped under:

- process: `sh` or the explicit shell process
- command family: `sh`
- observation source: shell bounded output or passthrough program

Shell opportunity rows distinguish line-window, generated-line, and combined
projections. Useful future dimensions are script size, remote versus local
shell, heredoc use, and bounded output status.

## Command Selection Guide

Use `cx sh` for local multi-line scripts.

Use `cx -- bash -lc` for one-line shell syntax.

Use `cx -- ssh <host> "bash -s"` for remote multi-line scripts.

Use `cx report <cx command...>` when CX appears to alter shell semantics or
produces clearly incorrect shell output.
