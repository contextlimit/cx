# CX Git Features

CX treats Git as a narrow, explicitly supported command family. The goal is not to
replace Git. The goal is to make the Git commands that agents call constantly
produce bounded, truthful output while keeping a raw recovery path when exact
evidence is required.

The supported Git surface is:

- `cx git status`
- `cx git diff`
- top-level `cx diff`
- `cx git log`
- `cx git show`
- `cx git evidence-diff`
- `cx git conflict-diff`

When using auto mode, the same clear shapes are available through `cx --`:

- `cx -- git status`
- `cx -- git diff`
- `cx -- git log`
- `cx -- git show`
- `cx -- git evidence-diff`
- `cx -- git conflict-diff`

Unsupported or parser-risky Git commands may fall through passthrough when the
local `passthrough_unsupported_commands` setting is enabled.

## Status

`cx git status` without additional arguments is converted into:

```sh
git status --porcelain -b
```

CX then parses the porcelain output into a compact working tree summary:

- current branch or detached head line
- staged paths
- modified paths
- untracked paths
- conflicted paths

This preserves the information agents usually need before editing or committing,
without printing the long human help text from normal `git status`.

`cx git status <args...>` is converted into:

```sh
git status <args...>
```

That form is treated as a user-directed status query. CX forwards the arguments
and applies only light output filtering.

Examples:

```sh
cx git status
cx -- git status
cx git status --short
cx -- git status --porcelain=v1
```

## Diff

`cx git diff` defaults to compact mode. CX first asks Git for a stat summary:

```sh
git diff --stat <args...>
```

Then it asks Git for the full diff:

```sh
git diff <args...>
```

CX derives a bounded summary from the real full diff. The compact rendering
keeps file names, hunk headers, added and removed line counts, and a limited
amount of hunk evidence. It does not invent diff content. If the output is
truncated, CX appends a recovery pointer:

```text
[full diff: cx git diff --no-compact]
```

The default compact diff exists because full unified diffs can dominate an agent
turn even when the useful question is just "what changed?" or "which files are
risky?"

These shapes bypass compact mode and return direct Git output:

```sh
cx git diff --stat
cx git diff --numstat
cx git diff --shortstat
cx git diff --name-only
cx git diff --name-status
cx git diff --raw
cx git diff --check
cx git diff --summary
cx git diff --quiet
cx git diff --exit-code
cx git diff --no-compact
```

They convert to:

```sh
git diff --stat <remaining args...>
git diff --numstat <remaining args...>
git diff --shortstat <remaining args...>
git diff --name-only <remaining args...>
git diff --name-status <remaining args...>
git diff --raw <remaining args...>
git diff --check <remaining args...>
git diff --summary <remaining args...>
git diff --quiet <remaining args...>
git diff --exit-code <remaining args...>
git diff <remaining args...>
```

`--no-compact` is a CX-only flag. CX strips it before invoking Git.
List/status/control modes bypass compacting because the exact stdout or exit
code is the evidence. For example, unresolved path probes stay raw:

```sh
cx -- git diff --name-only --diff-filter=U
```

The top-level command:

```sh
cx diff <args...>
```

uses the same implementation as:

```sh
cx git diff <args...>
```

This convenience applies only to direct CX syntax. In auto mode, use the
unambiguous Git family spelling:

```sh
cx -- git diff <args...>
```

Bare auto-mode `diff` is the native system command:

```sh
cx -- diff -qr left right
cx -- diff -u old.txt new.txt
```

CX sends those argv directly through passthrough. It does not reinterpret them
as Git Diff.

Pathspec separators are preserved for Git commands:

```sh
cx -- git diff -- src/lib.rs
cx -- git log -1 -- src/lib.rs
cx -- git show HEAD -- src/lib.rs
```

CX restores the command-level `--` after Clap parsing before invoking Git. This
prevents a missing path from being reinterpreted as a revision.

## Log

`cx git log` uses a compact CX history format unless the user supplies a native
format such as `--oneline`, `--pretty`, or `--format`.

Use exact native history output with:

```sh
cx -- git log --no-compact -1 --format=fuller HEAD
```

`--no-compact` is CX-only. CX removes it before invoking Git and does not inject
the default CX format, limit, or merge policy in this mode.

## No-Index Diff With Process Substitution

Shell process substitution is common when inspecting conflict stages or generated
content:

```sh
git diff --no-index --stat <(git show :2:path) <(git show :3:path)
```

When this kind of shape reaches CX as file-descriptor paths, CX recognizes the two
descriptor arguments for `git diff --no-index`:

- `/dev/fd/<n>`
- `/proc/self/fd/<n>`
- `/proc/<pid>/fd/<n>`

CX materializes exactly those two descriptor inputs into temporary files under:

```text
~/.cx/cache/git-no-index/
```

It then converts the command into:

```sh
git diff --no-index <materialized-left-file> <materialized-right-file>
```

The temporary files are removed when the diff operation finishes.

Git uses exit code `1` for a successful `--no-index` comparison when differences
exist. CX treats exit code `1` with diff output and no stderr as a successful
difference result, not as a tool failure.

This conversion is intentionally narrow. It only materializes exactly two
file-descriptor inputs for `git diff --no-index`. Other shell syntax belongs in
`cx sh` or `cx -- bash -lc`.

## Conflict Diff

`cx git conflict-diff` is the standard CX command for comparing conflict stages
without relying on shell process substitution.

The default shape:

```sh
cx git conflict-diff path/to/file.js
```

converts to:

```sh
cx git diff :2:path/to/file.js :3:path/to/file.js
```

Stage `2` is the left side and stage `3` is the right side by default.

The explicit stage shape:

```sh
cx git conflict-diff --stage 1:3 path/to/file.js
```

converts to:

```sh
cx git diff :1:path/to/file.js :3:path/to/file.js
```

Stat and raw modes are forwarded into the underlying diff:

```sh
cx git conflict-diff --stat path/to/file.js
cx git conflict-diff --no-compact path/to/file.js
```

These convert to:

```sh
cx git diff --stat :2:path/to/file.js :3:path/to/file.js
cx git diff --no-compact :2:path/to/file.js :3:path/to/file.js
```

Multiple paths are supported. CX runs the conversion per path and separates
sections with a path header so the output stays attributable:

```sh
cx git conflict-diff src/a.js src/b.js
```

Conceptually converts to:

```sh
cx git diff :2:src/a.js :3:src/a.js
cx git diff :2:src/b.js :3:src/b.js
```

Use `conflict-diff` instead of process substitution when the intended comparison
is "show me ours versus theirs for this conflicted path." It is more portable,
works through `cx --`, and records a clearer command shape in insights.

## Evidence Diff

`cx git evidence-diff` is the raw diff command intended for machine-consumed plan,
commit, and policy evidence. It deliberately avoids compacting the patch
because a downstream evaluator may need exact added and removed lines.

Default:

```sh
cx git evidence-diff
```

converts to a first-parent diff when `HEAD` has a parent:

```sh
git diff --no-ext-diff --no-color HEAD^..HEAD
```

If `HEAD` is a root commit, CX falls back to:

```sh
git show --format= --no-ext-diff --no-color --patch HEAD
```

This parent-aware default matters for merge commits because `git show --patch
HEAD` can emit no patch, while automated rule evaluators need a normal unified
diff to compute changed-file and changed-line metrics.

Single commit with a parent:

```sh
cx git evidence-diff abc1234
```

converts to:

```sh
git diff --no-ext-diff --no-color abc1234^..abc1234
```

Root commits still use `git show --format= --no-ext-diff --no-color --patch
<commit>` so initial import diffs remain available.

Range:

```sh
cx git evidence-diff HEAD~1..HEAD
```

converts to:

```sh
git diff --no-ext-diff --no-color HEAD~1..HEAD
```

Path filtering is accepted after `--`:

```sh
cx git evidence-diff HEAD -- src/lib.rs tests/recent_calls.rs
```

converts to:

```sh
git diff --no-ext-diff --no-color HEAD^..HEAD -- src/lib.rs tests/recent_calls.rs
```

For a range with path filtering:

```sh
cx git evidence-diff HEAD~1..HEAD -- src/lib.rs
```

converts to:

```sh
git diff --no-ext-diff --no-color HEAD~1..HEAD -- src/lib.rs
```

Use `evidence-diff` when the output will be sent as `diffText`, `unifiedDiff`, or
other exact rule evidence. Use normal `cx git diff` when the output is for
human or agent review and compact evidence is enough.

For a canonical machine-consumed handoff, write the raw patch directly from
the installed auto-mode command:

```sh
cx -- mkdir -p .tmp
cx -- git evidence-diff HEAD^..HEAD > .tmp/cx-exact-commit.diff
cx -- wc -c .tmp/cx-exact-commit.diff
```

When the receiving tool can read the checkout, pass that path as
`diffArtifactPath` or `patchArtifactPath`. When the MCP host is remote, send the
literal bytes from `cx -- git evidence-diff` as `diffText` or `unifiedDiff` together
with the commit hash, changed files, risk, and complexity evidence. Do not use a
normal compact `cx git diff`, `cx git show`, or a later compact read of the
artifact as the authoritative rule-evaluation payload.

The installed-wrapper and output-metric suites assert that auto-mode
`cx -- git evidence-diff <range>` emits the same patch bytes as the underlying raw
Git range. This contract is separate from the compact review behavior of
`cx git diff`.

## Log

`cx git log` without user formatting or an explicit parent policy converts to a
bounded, no-merge log:

```sh
git log --pretty=format:%h %s (%ar) <%an>%n%b%n---END--- -10 --no-merges
```

CX then formats the result so each commit remains scannable. Long bodies are
truncated. Merge commits are omitted by default because they usually add noise to
agent context.

Explicit count flags are honored:

```sh
cx git log -5
cx git log -n 20
cx git log --max-count=20
```

Parent-selection and traversal flags are authoritative:

```sh
cx git log --first-parent
cx git log --merges
cx git log --no-merges
cx git log --min-parents=2
cx git log --max-parents=1
cx git log --no-min-parents
cx git log --no-max-parents
```

When one of those policies is present, CX does not add a hidden `--no-merges`
filter. This matters for `--first-parent`, where merge commits often carry the
authoritative release or integration history.

User formatting is also authoritative. When a user passes `--oneline`,
`--pretty`, or `--format`, CX forwards the requested Git format and history
selection without applying its default pretty format or hidden merge filter.
Successful stdout and stderr retain the native stream termination.

For example:

```sh
cx -- git log -n 1 --oneline
cx -- git log -n 1 --format=%H
cx -- git log --first-parent --oneline --decorate -n 20
```

These forms select the same commits as the corresponding native Git commands.
CX's bounded no-merge behavior remains the default only for the compact
CX-formatted history view.

## Show

`cx git show <args...>` converts directly to:

```sh
git show <args...>
```

The command exists so auto mode can route clear `git show` usage through CX and
record telemetry. CX currently does not compact `git show` output. If
`--no-compact` is present, CX strips it as a CX-only compatibility flag before
invoking Git.

Use `cx git evidence-diff <commit>` rather than `cx git show <commit>` when the
patch is intended for exact machine-consumed rule evidence.

## Failure Artifacts And Insights

Git wrappers execute the real `git` binary. CX preserves the real exit code
unless it is handling Git's expected `--no-index` difference exit.

When a Git command exits nonzero, CX attempts to store a failure artifact under:

```text
~/.cx/cache/failures/git/
```

and appends a full-output hint when available:

```text
[full output: ...]
```

When insights recording is enabled, Git invocations are recorded in:

```text
~/.cx/db.sqlite
```

The command labels are intentionally specific enough to analyze Git at multiple
levels:

- process: `git`
- command family: `git diff`, `git status`, `git log`, `git show`,
  `git evidence-diff`, or `git conflict-diff`
- command: the readable command shape after CX normalization and redaction

This lets CX answer questions such as:

- how often agents ask for compact diffs
- how often exact evidence diffs are needed
- how many bytes and tokens compact diff saves
- how often conflict-stage comparisons happen
- how often process-substitution repair was needed

## Command Selection Guide

Use `cx git status` for a compact worktree summary.

Use `cx git diff` for bounded review output.

Use `cx git diff --no-compact` when the exact working tree diff is needed by a
human or a downstream tool.

Use `cx git evidence-diff` when submitting exact plan, policy, or commit-rule
evidence.

Use `cx git conflict-diff` when comparing conflict stages.

Use `cx git log` for compact history inspection.

Use `cx git show` when Git's native show behavior is specifically required.
