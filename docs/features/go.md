# CX Go Test Features

`cx go test` is the official Go test wrapper. It uses Go's structured JSON event
stream when possible and renders deterministic summaries.

The supported surface is:

```sh
cx go test <args...>
cx -- go test <args...>
```

## JSON Event Mode

When the user has not already requested JSON output, CX adds:

```sh
-json
```

Conceptually:

```sh
go test -json <args...>
```

The JSON event stream lets CX summarize package and test state without relying
only on text heuristics.

If the user already passed `-json`, CX does not add it again.

## Event Summary

CX parses Go test events into deterministic package and test state. The summary
tracks:

- package pass/fail status
- failed tests
- skipped tests
- build output
- non-JSON fallback lines
- elapsed and final package status where available

Package and test summaries are ordered deterministically.

## Build Failures

Go build failures can occur before normal test events complete. CX keeps build
failure output and non-JSON lines rather than dropping them.

## Output Contract

The Go wrapper may reduce:

- long passing event streams
- repeated package output
- routine success lines

It must preserve:

- real `go test` exit code
- failed package names
- failed test names
- build failure evidence
- non-JSON diagnostic lines
- final pass/fail package status

## Insights Labels

When insights recording is enabled, Go invocations are grouped under:

- process: `go`
- command family: `go test`

Useful future dimensions are whether CX injected `-json`, package count, failed
test count, build-failure count, and non-JSON fallback count.

## Command Selection Guide

Use `cx go test ./...` for broad Go proof when appropriate.

Use package or test narrowing for focused repair:

```sh
cx go test ./internal/api -run TestAuth
```

Use native Go output only when exact event stream text is the object of the
inspection.
