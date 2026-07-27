# CX TypeScript Compiler Features

`cx tsc` is the official TypeScript compiler wrapper. It focuses on finding the
real compiler and grouping diagnostics by file.

The supported surface is:

```sh
cx tsc <args...>
cx -- tsc <args...>
```

## Compiler Resolution

CX tries to run the real TypeScript compiler. It prefers project-local compiler
paths such as:

```text
node_modules/.bin/tsc
node_modules/typescript/bin/tsc
```

This avoids the npm placeholder package that prints a message telling users it
is not the real `tsc` command.

If no local compiler is found, CX falls back to normal PATH resolution.

## Diagnostic Grouping

TypeScript diagnostics often include file, line, column, error code, and message.
CX groups diagnostics by file and preserves codes such as:

```text
TS2304
TS2322
TS2345
```

Non-diagnostic compiler output is preserved so config errors, invocation errors,
and placeholder problems remain visible.

## Output Contract

The TypeScript wrapper may reduce:

- repeated compiler noise
- long diagnostic streams
- redundant file headings

It must preserve:

- real compiler exit code
- file paths
- line and column locations
- TypeScript error codes
- diagnostic messages
- non-diagnostic output that explains command or config failure

## Insights Labels

When insights recording is enabled, TypeScript invocations are grouped under:

- process: `tsc`
- command family: `tsc`

Useful future dimensions are compiler source category, diagnostic count, file
count, non-diagnostic output count, and placeholder avoidance.

## Command Selection Guide

Use `cx tsc --noEmit` for TypeScript proof.

Use project flags normally:

```sh
cx tsc -p tsconfig.json --noEmit
```

Use native `tsc` only when exact compiler stdout/stderr formatting is the thing
being tested.
