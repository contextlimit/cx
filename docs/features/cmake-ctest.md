# CX CMake And CTest Features

CX supports C++ build and test proof through `cmake build` and `ctest`.

The supported surface is:

```sh
cx cmake build <args...>
cx ctest <args...>
cx -- cmake --build <args...>
cx -- ctest <args...>
```

## CMake Build Mapping

The CX command:

```sh
cx cmake build <args...>
```

maps to:

```sh
cmake --build <args...>
```

This gives CX a stable command family while preserving native CMake build
semantics.

## Native Auto Mode

The native command shape:

```sh
cx -- cmake --build build-web --target sample-ui -j8
```

is normalized at the parser boundary to the official `cmake build` wrapper. The
normalization changes only CX's internal subcommand spelling: the child process
still receives `cmake --build build-web --target sample-ui -j8`, and telemetry keeps
the original readable argv while grouping the invocation under `cmake build`.
This route does not depend on unsupported-command passthrough being enabled.

Configure, install, and other CMake modes remain passthrough behavior until they
receive separate parser, forwarding, output, and installed-binary coverage.

## Multi-Target Builds

When the shape is safe, CX can split multiple targets into sequential builds.
The goal is to keep output attributable per target and preserve failure evidence.

If the argument shape is ambiguous, CX does not split. It runs the native CMake
build command and reports the real result.

## CTest Normal Mode

Normal CTest:

```sh
cx ctest --test-dir build -R unit
```

runs real `ctest` and summarizes:

- failing test names
- failure diagnostics
- final pass/fail status
- relevant output context

## CTest List Mode

List mode:

```sh
cx ctest -N
cx ctest --show-only
```

uses a separate bounded catalog formatter. The test list is the point of list
mode, so CX preserves visible test names instead of filtering it like normal test
execution.

## Output Contract

The CMake/CTest wrappers may reduce:

- routine successful build lines
- repeated progress output
- long CTest success logs
- huge test catalogs

They must preserve:

- real exit codes
- target or test attribution
- build errors
- failing test names
- failure diagnostics
- final result summaries

Failed CMake builds use a severity-aware evidence budget. CX gives compiler,
linker, CMake, Ninja, Make, and runtime/toolchain failure anchors priority over
warnings, keeps bounded context around those anchors, and retains a terminal
output window. Exact repeated warning lines are deduplicated with an explicit
suppression count; warnings with different paths, locations, versions, or text
remain distinct. Evidence is rendered in native line order, so an early compiler
failure and a later build-system stop remain causally readable.

Successful CMake builds have a dedicated output-metric fixture and Criterion hot
path. Tiny empty-success summaries are bounded and attributed with expansion
reason `build-result-summary` rather than appearing as unclassified expansion.

## Insights Labels

When insights recording is enabled, these invocations are grouped under:

- process: `cmake`
- command family: `cmake build`
- process: `ctest`
- command family: `ctest`

Both `cx cmake build ...` and `cx -- cmake --build ...` use the `cmake build`
family. This distinction matters when comparing official realized savings with
historical `passthrough cmake` opportunities.

Useful future dimensions are split target count, CTest list mode versus normal
mode, failure count, and build target names.

## Command Selection Guide

Use `cx cmake build` for CMake build proof.

Use `cx ctest -N` to inspect the available tests.

Use focused `cx ctest -R <pattern>` proof before broad test runs when repairing a
specific C++ behavior.
