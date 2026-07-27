# CX Read Features

`cx read` is the official file-reading command. It is designed to return bounded,
truthful file evidence without requiring an agent to dump an entire file into the
turn.

The supported surface is:

```sh
cx read <file>
cx read <file> --head <lines>
cx read <file> --tail <lines>
cx read <file> --range START:END
cx read <file> --full
cx read <file> --line-numbers
cx read <file> --raw
cx read <file> --mode normal
cx read <file> --mode aggressive
cx read <file> --mode smart
```

`--smart` is a shorthand for `--mode smart`.

## Normal Read

The normal shape:

```sh
cx read src/lib.rs
```

reads the file, applies the default bounded rendering, and may automatically use
aggressive mode when the file is large enough to exceed the auto-aggressive line
or byte thresholds.

Conceptually:

```text
read file -> choose normal or auto-aggressive rendering -> emit bounded evidence
```

Normal mode is useful for ordinary source files where the visible content is
small enough to inspect directly.

## Head, Tail, And Range

Head:

```sh
cx read src/lib.rs --head 80
```

emits the first 80 lines.

Tail:

```sh
cx read src/lib.rs --tail 80
```

emits the last 80 lines.

Range:

```sh
cx read src/lib.rs --range 120:180
```

emits lines 120 through 180.

Explicit filesystem ranges are streamed through `BufReader` instead of reading
the whole file into memory first. Stdin range reads still need to read stdin
before slicing because stdin is not seekable.

Untransformed ranges preserve the selected source bytes' line termination:
LF stays LF, CRLF stays CRLF, and an unterminated final line remains
unterminated. CX switches back to its own line-terminated presentation only
when it actually transforms the range, such as adding line numbers or
truncating a generated/blob line.

Range mode has different truthfulness expectations than summary modes. A range
means "show these lines", not "summarize this file." For that reason `--range`
conflicts with explicit summary modes and max-line controls.

## Line Numbers

Line numbers can be added with:

```sh
cx read src/lib.rs --range 120:180 --line-numbers
cx read src/lib.rs --range 120:180 -n
```

For explicit ranges, the displayed numbers start at the original file line, not
at 1.

## Full And Raw

`--full` asks CX not to use the normal head/tail summary window.

`--raw` asks CX not to truncate huge individual lines. Raw mode is useful when a
single generated line or encoded payload is the actual evidence. Non-raw output
truncates very long lines with an explicit marker so one generated blob cannot
overwhelm the agent turn.

Explicit ranges over human-authored source paths preserve long lines by default,
including declarations, C++ initializer entries, JSX, regular-expression
assertions, CSS declarations, embedded scripts, and structured string literals.
Generated/codegen paths keep the generic shape-aware bounding policy. The
generated blob-token guard is evaluated separately and wins for every path: a
source wrapper around a large base64, encoded, or minified payload token is still
truncated. This keeps source safe to edit without restoring the original
one-line payload flood.

Use raw mode deliberately. It can produce much larger output than normal read
modes.

## Aggressive Mode

Aggressive mode:

```sh
cx read src/lib.rs --mode aggressive
```

keeps high-signal structural lines such as declarations, imports, attributes,
doc comments, and test-looking lines. It is meant for orientation in large source
files, not exact line-by-line review.

Auto-aggressive mode uses byte and line thresholds. Byte checks happen first so
large generated files do not need expensive line counting before CX decides to
summarize.

Disable automatic aggressive mode with:

```sh
cx read src/lib.rs --no-auto-aggressive
```

## Smart Mode

Smart mode:

```sh
cx read src/lib.rs --mode smart
```

uses the configured smart-read command when available, or CX's local fallback
summary when not. The configured command comes from:

- `CX_SMART_READ_COMMAND`
- `~/.config/cx/config.toml` or `$XDG_CONFIG_HOME/cx/config.toml`

The environment variable wins over the config file.

Smart mode conflicts with raw, full, range, line-number, and explicit window
options because external or fallback summarization is not the same thing as
showing exact file lines.

## Output Contract

`cx read` may reduce:

- large source files
- generated-looking single-line blobs
- low-signal bodies in aggressive mode
- normal output that exceeds default windows

It must preserve:

- explicit requested line ranges
- LF, CRLF, and unterminated-final-line semantics for untransformed ranges
- line-number offsets for ranges
- raw huge lines when `--raw` is used
- visible truncation markers when content is omitted
- enough structural evidence in aggressive summaries to orient the user

## Insights Labels

When insights recording is enabled, read invocations are grouped under:

- process: `read`
- command family: `read`
- command examples: `read`, `read range`, `read head`, `read tail`

Useful future analysis dimensions are read mode, window type, raw versus non-raw,
line count, byte count, and whether auto-aggressive mode was used.

## Command Selection Guide

Use `cx read --range` for exact source evidence.

Use `cx read --head` or `--tail` for file boundaries.

Use `cx read --mode aggressive` for large-file orientation.

Use `cx read --raw` only when preserving huge lines matters.

Use `cx read --mode smart` when a summary is acceptable and the configured smart
read behavior is trusted for the task.
