# CX Container Features

CX supports a narrow set of container and cluster inspection commands:

```sh
cx docker ps <args...>
cx docker logs <container> <args...>
cx kubectl logs <pod> <args...>
```

The same clear shapes can be used through auto mode:

```sh
cx -- docker ps
cx -- docker logs api
cx -- kubectl logs api-pod
```

## Docker Ps

`docker ps` supports Docker global arguments before the `ps` subcommand:

```sh
cx -- docker --context prod ps
```

If the user does not provide a custom `--format`, CX injects a stable format and
compacts container rows.

If the user provides a custom format, CX does not try to parse every possible
template. It bounds the output through fallback windows.

## Docker Logs

`cx docker logs` preserves user arguments and defaults to a bounded tail when the
user did not specify a tail:

```sh
docker logs --tail 100 <container>
```

The wrapper uses log summarization to group repeated warnings and errors while
keeping useful context.

## Kubectl Logs

`cx kubectl logs` follows the same high-output log principle:

```sh
kubectl logs --tail 100 <pod>
```

unless the user supplied tail or other log-shaping arguments.

## Output Contract

The container wrappers may reduce:

- long container lists
- repeated log warnings
- repeated log errors
- huge default log output

They must preserve:

- real Docker or kubectl exit codes
- container or pod identity
- recent log evidence
- repeated warning/error signatures
- custom-format output through a bounded fallback rather than misleading parsing

## Insights Labels

When insights recording is enabled, invocations are grouped under:

- process: `docker`
- command family: `docker ps` or `docker logs`
- process: `kubectl`
- command family: `kubectl logs`

Useful future dimensions are custom format usage, injected tail usage, log error
group count, warning group count, and displayed line count.

## Command Selection Guide

Use `cx docker ps` for compact container inventory.

Use `cx docker logs <container>` or `cx kubectl logs <pod>` for recent bounded
logs.

Pass explicit `--tail`, `--since`, or selectors when the default recent window is
not the right evidence.
