# CX Node Features

CX supports Node in two related ways:

- official syntax checking through `node --check` or `node -c`
- bounded runtime execution for clear Node runtime shapes routed through CX

The supported surface includes:

```sh
cx node --check <files...>
cx node -c <files...>
cx -- node --check <files...>
cx -- node <script-or-runtime-args...>
```

## Syntax Check

Syntax check:

```sh
cx node --check src/app.js
cx node -c src/app.js
```

delegates ordinary JavaScript files to:

```sh
node --check <file>
```

Multiple files are supported:

```sh
cx node --check a.js b.mjs c.cjs
```

CX checks each file and returns a nonzero exit code if any file fails.

## JSX Syntax

`.jsx` files are parsed internally with OXC because native Node syntax checking
does not consistently accept JSX syntax or extension behavior.

This is still syntax checking, not transformation or runtime execution.

## Runtime Node Commands

Clear runtime commands through auto mode can be bounded by CX:

```sh
cx -- node app/tests/example.mjs
cx -- node --input-type=module
```

Stdin `--input-type` forms are recognized so heredoc-based scripts can stay
attached to CX telemetry without being rejected as unsupported syntax-check
shapes.

Runtime execution is not the same feature as `node --check`. It preserves the
real Node exit code and bounds output when command optimizations are enabled.

The position of `--check` is significant. CX treats it as syntax-check mode
only when it appears before the program path:

```sh
cx -- node --check script.mjs
```

When a script accepts its own `--check` option, CX preserves native runtime
semantics and forwards the argv unchanged:

```sh
cx -- node tools/generate_contracts.mjs --check
```

The second form is recorded as `node run`, not `node check`. Direct
`cx node script.mjs --check` is rejected with guidance because the official
`cx node` surface remains syntax-check-only.

## Output Contract

The Node wrapper may reduce:

- long runtime stdout/stderr
- huge one-line generated JSON, HTML/XML, or blob-like runtime payloads
- repeated success output across multiple syntax checks

Generated one-line runtime payloads retain both ends and include the original
character count in the truncation marker. Long source, JSX, CSS, regex, and
ordinary prose remain intact. Setting `command_optimizations=false` preserves
the full runtime output.

It must preserve:

- real Node exit codes
- syntax error file locations
- syntax error messages
- per-file success or failure for multi-file checks
- runtime stderr needed to diagnose failures

CX deliberately preserves Node's current syntax truth, including whatever the
installed Node version reports for import assertions, import attributes, and
other evolving JavaScript syntax.

## Insights Labels

When insights recording is enabled, Node invocations are grouped under command
families such as:

- `node check`
- `node run`
- `node test`

Useful future dimensions are syntax-check file count, JSX parser path, runtime
versus check mode, stdin input-type usage, and bounded-output status.

## Command Selection Guide

Use `cx node --check` for syntax proof.

Use `cx -- node <script>` when you need to execute a Node script through CX auto
mode.

Use `cx sh` for shell syntax around Node, such as heredocs with environment
assignments or redirects:

```sh
cx sh <<'BASH'
node --input-type=module <<'NODE'
console.log('ok');
NODE
BASH
```
