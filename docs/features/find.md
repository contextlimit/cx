# CX Find Features

`cx find` is a bounded discovery command. It is not a complete POSIX `find`
expression evaluator.

The supported surface includes:

```sh
cx find <roots...>
cx find <root> -maxdepth <n>
cx find <root> -type f
cx find <root> -type d
cx find <root> -name '*.rs'
cx find <root> -iname '*.md'
cx find <root> -path '*/tests/*'
cx find <root> --hidden
cx find <root> --max-results <n>
```

Auto mode supports clear shapes:

```sh
cx -- find src -maxdepth 2 -type f -name '*.rs'
```

Auto mode sends native boolean expressions to passthrough instead of applying
the bounded matcher:

```sh
cx -- find build -type f '(' -name one -o -name two ')' -print
```

This rule applies to `(`, `)`, `-o`, `-or`, `-a`, and `-and`.

## Predicate Support

CX supports these discovery predicates:

- `-maxdepth <n>`
- `-type f`
- `-type d`
- `-name <glob>`
- `-iname <glob>`
- `-path <glob>`
- `-ipath <glob>`
- `-wholename <glob>`
- `-iwholename <glob>`
- `-perm <mode>`
- `--hidden`
- `--max-results <n>`

Name and path predicates use glob matching. Results are sorted deterministically.
Repeated filename predicates are alternatives within one filename group, and
repeated path predicates are alternatives within one path group. When a clear
bounded-discovery shape supplies both groups, the entry must match both. For
example, `-path '*/build/*' -name sample-service` cannot return every file under
`build` or matching files outside `build`.

## Accepted No-Op Syntax

CX accepts some common find syntax tokens so familiar command shapes do not fail
immediately:

- `(`
- `)`
- `-o`
- `-a`
- `--`
- `-print`
- `-print0`

These tokens do not mean direct `cx find` implements full boolean expression
semantics.

Example:

```sh
cx find src -type f '(' -name '*.rs' -o -name '*.toml' ')' -print
```

is accepted by direct `cx find` as a bounded discovery shape, but it should not
be treated as a full POSIX boolean expression. The equivalent `cx -- find`
shape uses native passthrough when enabled. If exact POSIX Find behavior
matters, use `cx -- find` or `cx sh`.

## Hidden Paths

Hidden paths are skipped by default. Use:

```sh
cx find . --hidden -name '.env*'
```

when hidden files or directories are the target.

## Unsupported Actions

CX does not support actions such as:

```sh
find . -exec ...
find . -delete
find . -printf ...
```

Use `cx sh` or passthrough for those native `find` behaviors.

## Output Contract

`cx find` may reduce:

- large directory traversals
- repeated output from deep trees
- hidden noise unless requested

It must preserve:

- deterministic ordering
- path text for displayed results
- omitted-result counts when output is capped
- the explicit root and filter intent of clear bounded-discovery shapes

Traversal failures are not treated as empty trees. A missing root, unreadable
entry, or permission-metadata failure produces bounded deterministic stderr and
a nonzero exit code. Matches discovered from other valid roots remain on
stdout, so partial evidence is recoverable without presenting the traversal as
complete. CX shows at most 20 traversal diagnostics and reports the omitted
error count.

## Insights Labels

When insights recording is enabled, find invocations are grouped under:

- process: `find`
- command family: `find`

Useful future dimensions are root count, max depth, file versus directory type,
name/path predicate count, hidden mode, and max-result cap.

## Command Selection Guide

Use `cx find` to discover files and directories by name, path, type, and depth.

Use `cx grep --files` when the real goal is "list searchable files."

Use `cx -- find` or `cx sh` for `-exec`, `-delete`, full boolean logic, or exact
POSIX Find behavior.
