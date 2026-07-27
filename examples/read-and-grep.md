# Read and Grep Examples

CX distinguishes exact requested evidence from broad output that benefits from a
bounded projection.

## File Reads

```sh
cx -- read src/lib.rs
cx -- read --head 80 src/dispatch.rs
cx -- read --range 120:220 src/commands/read/mod.rs
cx -- sed -n '120,220p' src/commands/read/mod.rs
cx -- nl -ba src/lib.rs
cx -- read --mode aggressive src/support/runner.rs
cx -- read --mode smart src/support/runner.rs
```

Explicit ranges remain exact. Requested Markdown, prose, tabular documents,
diffs, patches, JSON files under a `plans` directory, source code, JSX, CSS, and
regex-heavy lines are protected from generated-blob truncation.

Use raw mode only when exact long-line bytes are intentional:

```sh
cx -- read --range 1:5 --raw generated/payload.txt
```

## Grep And Rg

```sh
cx -- rg -n "smart_read|ReadMode::Smart" src
cx -- rg -n -C 2 "fallback_window" src
cx -- grep -n -e '--no-compact' src/cli.rs
cx -- rg -F -e 'literal(value)' -e 'another|literal' src
cx -- rg --files src tests
```

The dialects are deliberate:

- `cx -- grep` follows basic-grep expectations by default.
- `cx -- grep -E` uses extended regular expressions.
- `cx -- rg` uses ripgrep-style extended expressions.
- repeated `-F -e` is the safest path for shell-sensitive literal searches.

Bound the displayed evidence without changing total-match accounting:

```sh
cx -- rg -n --max-results 25 'error|warning|failed' src tests
```

Use `cx report` when a successful search result is clearly wrong or misleading:

```sh
cx report cx -- rg -n 'route|path' tests
```
