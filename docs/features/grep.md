# CX Grep And Rg Features

CX supports search through `grep`, with `rg` as a visible alias. The two command
names intentionally do not mean the same thing.

The supported surface includes:

```sh
cx grep <pattern> <paths...>
cx grep -E <pattern> <paths...>
cx grep -F -e <literal> <paths...>
cx grep --files <paths...>
cx grep --glob '<glob>' <pattern> <paths...>
cx grep --max-results <n> <pattern> <paths...>
cx grep --no-compact <pattern> <paths...>
cx rg <pattern> <paths...>
cx rg --no-compact <pattern> <paths...>
```

Auto mode supports clear shapes:

```sh
cx -- grep <pattern> <paths...>
cx -- rg <pattern> <paths...>
cx -- rg --no-compact <pattern> <paths...>
```

`--no-compact` is a CX-only search output control. CX strips it before invoking
the backend and preserves the backend stdout and stderr with native stream
termination. This includes empty stdout for native no-match exit 1 and long
generated match lines that ordinary search formatting would bound. Command
identity, exit codes, failure artifacts, and raw/emitted metrics are still
recorded.

Use explicit `-e` when the literal search pattern is `--no-compact`:

```sh
cx rg -F -e '--no-compact' src
```

Native-only ripgrep options that are not part of the official CX search
contract stay direct passthrough in auto mode. For example:

```sh
cx -- rg --max-count 20 -n 'needle' src
cx -- rg -o --pcre2 'translateY\\(50px\\)' steamui.css
cx -- rg -o --no-filename '"preview/[A-Za-z0-9_./-]+"' src
```

CX forwards this argv unchanged instead of treating `--max-count` as a search
pattern. Native regex-engine flags such as `--pcre2`,
`--auto-hybrid-regex`, and `--engine=pcre2` are likewise routed directly so
CX never mistakes those options for flag-looking search patterns.
`--no-filename` is also treated as a native output option instead of being
rewritten into a leading-dash search pattern.

## Dialect Contract

Default `cx grep` is basic-grep-like. In that mode, characters such as `(`, `)`,
`|`, `{}`, `+`, and `?` are treated like grep defaults where appropriate.

`cx grep -E` opts into extended regular expressions.

`cx rg` is ripgrep-like and extended by default.

Fixed-string search:

```sh
cx grep -F -e 'literal|not-regex' src
```

is the safest shape for shell-sensitive or regex-sensitive literals.

## Pattern And Path Splitting

`cx grep` requires an explicit pattern unless `--files` is used.

These are accepted:

```sh
cx grep -e '--flag-name' src
cx grep --regexp '--flag-name' src
cx grep -F -e 'a|b' -e 'c+d' src
```

The parser preprocesses patterns that start with `--` so users can search for
flag-looking text without the pattern being mistaken for a CX flag.

## Files Mode

File-list mode:

```sh
cx grep --files src
```

uses ripgrep files mode when available, or a fallback file collector when not.
Output is bounded and deterministic.

## Globs And Hidden Files

Glob filtering:

```sh
cx grep --glob '*.rs' 'fn run' src
cx grep -g '*.md' 'CX' docs
```

passes glob expectations into the search backend.

Hidden files are skipped by default unless the backend and options allow them:

```sh
cx grep --hidden 'pattern' .
```

## Context Output

Context flags are supported:

```sh
cx grep -A 3 'error' src
cx grep -B 3 'error' src
cx grep -C 3 'error' src
```

Context output is less structured than normal match grouping. CX uses bounded raw
fallback windows so context searches do not flood the turn.

## Match Formatting

Small searches with one through eight non-empty result lines stay in the native
grep/ripgrep location format when `--max-results` is absent. This preserves
leading source indentation and avoids expanding one useful result into CX
summary headers. Every line still passes through the generated-payload guard.

Larger match output is grouped deterministically by file. CX tracks total
matches and stores only displayed evidence when `--max-results` is active.
Explicit `--max-results` continues to use structured accounting even when the
backend happened to return eight or fewer lines, so the requested cap cannot be
silently bypassed.

When `compact_document_search_results=false`, searches that return only
recognized document or tabular text paths remain exact. This includes Markdown,
plain text, diffs, patches, CSV, and TSV.

Huge generated/blob match lines are truncated with explicit markers. Long
human-authored source lines use the same classifier as `cx read`: declarations,
statements, JSX, CSS, regex assertions, embedded scripts, and structured string
literals remain exact unless a generated payload token triggers the blob guard.
This applies to grouped matches and location-prefixed raw/context lines.

No-match output is explicit. When a default basic grep search contains a likely
extended-regex operator such as a bare `|`, CX can hint that `cx grep -E` or
`cx rg` may match the user's intent better.

## Backend Behavior

CX prefers `rg` when available. If `rg` fails with a regex parse error and the
search is not fixed-string, CX may retry through the grep fallback path.

That fallback is not a license to blur dialects. The command name and flags still
define the intended semantics.

## Output Contract

The grep wrapper may reduce:

- long match lists
- huge individual match lines
- raw context output
- file lists

It must preserve:

- the command's selected regex dialect
- match file attribution
- line numbers when requested
- total match counts where available
- visible truncation markers
- no-match evidence and relevant hints

When `--no-compact` is present, native output itself is the evidence and CX
does not add the normal no-match summary or formatting hints.

## Insights Labels

When insights recording is enabled, search invocations are grouped under:

- process: `grep` or `rg`
- command family: `grep`, `grep files`, or rg-equivalent search family
- observation source: backend-specific match or fallback path

Useful future dimensions are regex dialect, fixed-string mode, files mode,
context mode, fallback backend, and no-match hints.

## Command Selection Guide

Use `cx grep` for basic grep behavior.

Use `cx grep -E` for extended regex under the grep command name.

Use `cx rg` for ripgrep-like regex expectations.

Use repeated `-F -e` for exact literals.

Use `cx report <cx command...>` when CX returns clearly incorrect search results
for a command shape that should be supported.
