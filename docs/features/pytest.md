# CX Pytest Features

`cx pytest` is the official Python test wrapper. It keeps pytest proof output
bounded while preserving failure evidence.

The supported surface is:

```sh
cx pytest <args...>
cx -- pytest <args...>
```

## Command Execution

The wrapper prefers:

```sh
pytest <args...>
```

If `pytest` is not available, CX can fall back to:

```sh
python -m pytest <args...>
```

The real pytest exit code is preserved.

## Default Flag Injection

When the user has not specified traceback behavior, CX adds:

```sh
--tb=short
```

When the user has not specified verbosity behavior, CX adds:

```sh
-q
```

These defaults are intended for agent proof: less routine output, enough failure
context.

If the user explicitly passes traceback or verbosity flags, CX respects the user
shape instead of overriding it.

## Failure Summary

CX scans pytest output for useful proof evidence:

- failing test names
- assertion messages
- short traceback frames
- relevant file and line locations
- final pytest summary lines
- skipped, failed, and passed counts when present

Warnings and long captured output may be reduced when they do not affect the
failure evidence.

## Output Contract

The pytest wrapper may reduce:

- successful test listings
- long captured stdout/stderr blocks
- repeated warnings
- verbose traceback noise when CX injected short traceback mode

It must preserve:

- pytest's exit code
- failing test identifiers
- assertion or exception evidence
- source locations needed to repair the failure
- final result summaries

## Insights Labels

When insights recording is enabled, pytest invocations are grouped under:

- process: `pytest` or `python`
- command family: `pytest`

Useful future dimensions are whether CX injected `--tb=short`, whether it
injected `-q`, whether the fallback `python -m pytest` path was used, and failure
count.

## Command Selection Guide

Use `cx pytest` for normal Python proof.

Pass explicit pytest flags when you need native verbosity:

```sh
cx pytest -vv --tb=long tests/test_api.py
```

Use narrower selectors when a broad pytest run would be too expensive:

```sh
cx pytest -q tests/test_api.py -k auth
```
