# CX Runner Features

The runner is the process boundary used by CX command wrappers. Users do not call
it directly, but every wrapper depends on it.

The key implementation lives in:

```text
src/support/runner.rs
```

## File-Backed Capture

CX captures child stdout and stderr by redirecting them to files under:

```text
~/.cx/cache/capture/
```

instead of using pipe-backed `Command::output()`.

This avoids a known hang class where a direct child exits but a descendant
process inherits stdout or stderr descriptors. Pipe-backed output can wait
forever in that situation. File-backed capture lets CX wait for the direct child,
read the files, and clean them up.

## Run Filtered

Command wrappers usually call `run_filtered`. That path:

1. runs the real command through file-backed capture
2. applies the command-specific filter
3. preserves the real exit code
4. stores failure artifacts on nonzero exits when possible
5. appends full-output hints when artifacts are available

## Failure Artifacts

Failure artifacts live under:

```text
~/.cx/cache/failures/<tool>/
```

They are the recovery path when compact stderr/stdout is not enough to diagnose a
failure.

The user-visible hint looks like:

```text
[full output: ~/.cx/cache/failures/...]
```

CX creates that artifact only when captured stdout or stderr contains at least
one byte. A nonzero command with exactly empty stdout and stderr keeps its real
exit code and failure telemetry, but it does not create a zero-byte artifact or
emit a recovery pointer. Whitespace is still output and remains recoverable.

## Stdin And Timeout Capture

CX also has a stdin-fed capture path with timeout handling and a writer thread.
It is used for helper flows that need to feed a child process through stdin.

The child result and stdin delivery have an explicit precedence rule:

- if the child exits successfully, CX requires the stdin writer to finish
  successfully before accepting the result;
- if the child exits nonzero after closing stdin early, CX preserves that real
  exit code and captured stderr/stdout instead of replacing the child failure
  with a secondary broken-pipe write error;
- a writer-thread panic remains an internal capture failure in either case.

This matters for plugins and passthrough commands that reject input immediately.
The command's own failure evidence remains the useful diagnosis, while a
successful command cannot claim to have processed a request that was not fully
delivered.

## Output Contract

The runner must preserve:

- real child exit codes
- stdout and stderr separation
- nonzero child evidence when an early stdin close also causes a writer error
- fail-closed behavior when a successful child did not receive complete stdin
- recoverable failure output when output exists
- exact empty output without a synthetic artifact line
- cleanup of temporary capture files
- descriptor-inheritance safety

The runner must not be replaced by production `Command::output()` without a very
strong, benchmark-backed reason and equivalent hang coverage.

## Validation

Repo hygiene tests check that production Rust does not use raw
`Command::output()`.

Descriptor-inheritance tests cover grep and fallback paths that previously could
hang with pipe-backed capture.

Stdin tests force early child-side pipe closure with large payloads so both the
nonzero-child and successful-child precedence rules are deterministic.

## Insights Labels

Runner behavior is not usually a command family, but it affects all observation
records. Failure artifacts and raw/emitted output metrics depend on this
boundary.

Useful future dimensions are artifact availability, capture cleanup failures,
timeout count, and fallback-window usage.
