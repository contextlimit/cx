# Container and Cluster Examples

CX keeps container inventories and logs bounded without changing the native
command verdict.

## Docker

```sh
cx -- docker ps
cx -- docker --context staging ps
cx -- docker ps --format '{{.Names}}\t{{.Status}}'
cx -- docker logs api
cx -- docker logs --tail 250 api
```

For the standard `docker ps` shape, CX injects a stable row format and compacts
the inventory. A user-supplied `--format` remains authoritative and is bounded
only by generic output windows. `docker logs` defaults to the latest 100 lines
when no tail is supplied.

## Kubernetes

```sh
cx -- kubectl logs web-7d4c6f
cx -- kubectl logs --tail 250 web-7d4c6f
cx -- kubectl logs -n production deployment/web --all-containers
```

`kubectl logs` also defaults to the latest 100 lines. Repeated warnings and
errors are summarized while representative evidence and the real exit code are
preserved.

Use unsupported passthrough for command trees CX does not officially understand:

```sh
cx insights settings --set passthrough_unsupported_commands=true
cx -- docker inspect api
cx -- kubectl get pods -A
```

Passthrough output remains native and exact. CX may record an opportunity
estimate, but it does not apply that estimate to the response.
