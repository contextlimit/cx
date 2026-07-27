# CX Parser And Dispatch Features

The parser and dispatch layer define which commands are official CX command
families and how auto mode routes clear command shapes.

The main files are:

```text
src/cli.rs
src/dispatch.rs
```

## Command Name

The CLI command name is:

```sh
cx
```

`Cli::parse_cx()` calls the CX argument preprocessor before Clap parses the final
command enum.

## Auto Mode

Auto mode uses:

```sh
cx -- <command...>
```

For clear official shapes, auto mode routes to official wrappers:

```sh
cx -- git diff
cx -- rg pattern src
cx -- cargo test
```

For unsupported shapes, auto mode falls back to passthrough only when the local
setting allows it.

Some parser-risky shapes intentionally avoid official routing and use
passthrough so CX does not misinterpret native command semantics.

Family-specific CX output controls are parsed before flag-looking-pattern
repair. For example, `cx -- rg --no-compact -n pattern src` uses the official
search wrapper and keeps exact backend output. An explicit pattern value still
wins, so `cx -- rg -F -e '--no-compact' src` searches for that literal text.

The top-level `diff` name is intentionally mode-sensitive. Direct `cx diff`
remains the Git diff convenience command. In auto mode, bare `diff` means the
native executable, while Git uses the unambiguous family spelling:

```sh
cx diff --stat
cx -- git diff --stat
cx -- diff -qr left right
```

The first two use the official Git wrapper. The last command uses native
passthrough with the original `diff` argv.

Auto-mode Find expressions containing parentheses or explicit boolean
operators also use native passthrough. CX's bounded Find wrapper does not
implement full expression semantics:

```sh
cx -- find build -type f '(' -name one -o -name two ')' -print
```

Parse errors are not all passthrough candidates. Unknown tools, unsupported
subcommands of native tools, and parser-risky native argument forms may use
passthrough. Argument conflicts, invalid values, missing values, and invalid
flags on CX-owned roots such as `read`, `insights`, and `sh` remain CX parser
errors. This prevents an invalid official command from silently invoking an
unrelated executable with the same name.

For example, `--head` and `--tail` are mutually exclusive read windows:

```sh
cx -- read --head 5 --tail 5 file.txt
```

The command reports the CX argument conflict. It does not execute the system
`read` program.

When invocation recording is enabled, final parser rejections are written to
the `command_routing_decisions` insights table. The row records whether the
shape was eligible for passthrough, whether passthrough was enabled, the stable
Clap error kind, and a reason such as `cx-owned-parse-error` or
`passthrough-disabled`. It does not count as an executed command invocation.
Inspect this evidence with:

```sh
cx insights routing --limit 20
```

Command text remains optional and follows the existing insights privacy
settings. Recording-disabled CX still exits with the same parser error and does
not create the insights database.

## Official Families

Official families include:

- `git status|diff|log|show|evidence-diff|conflict-diff`
- top-level `diff`
- `read`
- `grep` and visible alias `rg`
- `ls`
- read-like `cat`, `head`, `tail`, `sed`, `nl`
- `pytest`
- `cargo test`
- `go test`
- `tsc`
- `node`
- `sh`
- `cmake build`
- `ctest`
- `find`
- `docker ps|logs`
- `kubectl logs`
- `report`
- `insights`

Unsupported commands are not official support even if passthrough runs them.

## Trailing Arguments

Many native tools accept flags after subcommands or positional values that begin
with `-`. CX uses trailing var args and hyphen-value support where needed so
natural command shapes parse correctly.

Examples:

```sh
cx ctest -N -R unit
cx find . -maxdepth 2 -type f -name '*.rs'
cx node --check --experimental-syntax file.js
```

## Grep Preprocessing

Grep patterns can look like flags:

```sh
cx grep -e '--help' src
```

The parser preprocesses these cases so users can search for flag-like text
naturally.

## Dispatch Boundary

`dispatch.rs` maps parser enums into command modules. The dispatch layer should
stay thin:

- translate CLI structs into command option structs
- call the command module
- record observations for insights
- avoid centralizing a generic proxy framework

Command-specific behavior belongs in command modules.

## Insights Identity

Dispatch records identity for analysis:

- process/root command
- command family
- readable command shape
- argv JSON when enabled
- raw and emitted observations

This is why `git`, `git diff`, and a specific redacted `git diff -- path` can be
analyzed at different levels.

## Command Selection Guide

Use explicit CX command families when you know the official shape.

Use `cx -- <command...>` in this environment for auto mode and passthrough
measurement.

Use `cx sh` or `cx -- bash -lc` when shell syntax is required.

Do not add a new official command family without parser coverage, fake-binary
tests, output metrics where compaction matters, and installed-binary smoke
coverage.
