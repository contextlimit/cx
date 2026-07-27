# Git Examples

Use compact Git output for human review and exact Git output when another tool
must consume the patch or status bytes.

## Working Tree Review

```sh
cx -- git status
cx -- git diff
cx -- git diff -- src/lib.rs src/dispatch.rs
cx -- git diff --stat
cx -- git diff --name-only --diff-filter=U
cx -- git diff --check
```

`git diff` is compact by default. Stat, name, raw, check, quiet, and other
machine-oriented modes stay exact. Use `--no-compact` for a full human-readable
unified diff:

```sh
cx -- git diff --no-compact -- src/lib.rs
```

## History

```sh
cx -- git log -n 20 --oneline
cx -- git log --no-compact -1 --format=fuller HEAD
cx -- git show --stat HEAD
```

User-supplied Git formatting remains authoritative. `--no-compact` disables
CX's default history projection.

## Exact Patch Evidence

```sh
mkdir -p .tmp
cx -- git evidence-diff HEAD^..HEAD > .tmp/cx-exact-commit.diff
wc -c .tmp/cx-exact-commit.diff
```

`evidence-diff` emits the raw no-color patch. With no range, it chooses a
first-parent diff for ordinary commits and a root-safe `git show` form for an
initial commit.

## Conflict Stages

```sh
cx -- git conflict-diff --stat src/config.rs
cx -- git conflict-diff --no-compact src/config.rs
cx -- git conflict-diff --stage 1:3 src/config.rs
```

This compares Git index stages without shell process substitution, temporary
named files, or ambiguous `/dev/fd` handling.
```
