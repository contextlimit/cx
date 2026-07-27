# CX Ls Features

`cx ls` is a compact directory inventory wrapper. It is intended for orientation,
not exact reproduction of every `ls` flag.

The supported surface is:

```sh
cx ls
cx ls <path>
cx ls <args...>
cx -- ls <args...>
```

## Default Listing

The default shape:

```sh
cx ls
```

invokes a detailed listing internally and renders a compact summary of
directories and files.

Conceptually:

```sh
ls -la <path>
```

then:

```text
parse entries -> group directories and files -> summarize sizes and extensions
```

## Suppressed Development Noise

Common high-volume development directories are suppressed unless the command
shape asks to show them. Examples include:

- `.git`
- `node_modules`
- `target`
- `__pycache__`

The goal is to make the first directory inventory useful in source trees without
burying real files under cache entries.

## User Arguments

User arguments are forwarded where CX can still interpret the resulting listing.
Custom or unusual output shapes may fall back to bounded raw output.

Examples:

```sh
cx ls src
cx -- ls -la docs/features
```

## Output Contract

`cx ls` may reduce:

- long raw listing rows
- cache directory entries
- repeated metadata that does not help orientation

It must preserve:

- visible file and directory names that remain after filtering
- enough size/type information to orient the user
- an escape path through native `ls` or passthrough when exact output is needed

## Insights Labels

When insights recording is enabled, ls invocations are grouped under:

- process: `ls`
- command family: `ls`

Useful future dimensions are displayed entry count, suppressed directory count,
path count, and fallback versus structured output.

## Command Selection Guide

Use `cx ls` to orient in a project directory.

Use `cx find` for deterministic discovery by predicate.

Use raw `ls` through passthrough or `cx sh` if exact platform `ls` formatting is
the point of the command.
