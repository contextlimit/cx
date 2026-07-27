# Build and Test Examples

CX keeps the real child exit code and retains failure diagnostics while reducing
routine successful output.

## Rust

```sh
cx -- cargo test
cx -- cargo test parser::tests::leading_dash runner::tests::inherited_fd -- --exact
cx -- cargo test -p cx --lib
```

The second command is a safe multi-filter shape. Native Cargo accepts one test
filter, so CX runs one valid Cargo command per filter and returns nonzero if any
child run fails.

## Python And Go

```sh
cx -- pytest -q tests/test_parser.py
cx -- pytest -q -k 'parser or runner'
cx -- go test ./...
```

Pytest failures retain failing test names, assertion evidence, and the final
summary. Go test uses JSON events internally so package and test results remain
deterministically attributable.

## TypeScript And Node

```sh
cx -- tsc --noEmit
cx -- node --check src/app.js src/panel.jsx
cx -- node app/tests/smoke.mjs
```

CX parses `.jsx` syntax internally for `node --check`, avoiding Node-version and
extension differences. Clear Node runtime commands preserve native execution
and use bounded output when command optimizations are enabled.

## CMake And CTest

```sh
cx -- cmake --build build --target shell-tests
cx -- ctest --test-dir build --output-on-failure
cx -- ctest --test-dir build -N
```

CTest list mode preserves the requested test catalog instead of replacing it
with a pass/fail summary.

On nonzero exit, preserve the emitted recovery pointer:

```text
[full output: ~/.cx/cache/failures/<tool>/<artifact>.log]
```
