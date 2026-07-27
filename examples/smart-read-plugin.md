# Smart Read Plugin Example

Smart read is optional. Without a plugin, CX produces its deterministic local
summary. A configured plugin receives file context on stdin and must return one
JSON object on stdout.

## Configure A Plugin

```sh
export CX_SMART_READ_COMMAND=$HOME/bin/cx-smart-read-helper
cx -- read --mode smart src/commands/read/mod.rs
```

Or configure it in `~/.config/cx/config.toml`:

```toml
[smart_read]
command = "/absolute/path/to/cx-smart-read-helper"
timeout_ms = 5000
```

`CX_SMART_READ_COMMAND` takes precedence over the config file.

## Request Contract

The helper receives UTF-8 JSON on stdin:

```json
{
  "file": "src/commands/read/mod.rs",
  "cwd": "/path/to/project",
  "language": "rust",
  "content": "use std::path::Path;\n...",
  "max_lines": 12,
  "mode": "smart"
}
```

The helper returns:

```json
{"summary":"Purpose: routes read modes and preserves exact range semantics."}
```

The `summary` must be a non-empty string.

## Minimal Node Helper

Save this as an executable local helper:

```js
#!/usr/bin/env node

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  input += chunk;
});
process.stdin.on("end", () => {
  const request = JSON.parse(input);
  const firstLine = request.content.split(/\r?\n/, 1)[0].trim();
  process.stdout.write(JSON.stringify({
    summary: `${request.language} file: ${request.file}\nFirst line: ${firstLine}`,
  }));
});
```

Then:

```sh
chmod +x "$HOME/bin/cx-smart-read-helper"
CX_SMART_READ_COMMAND="$HOME/bin/cx-smart-read-helper" \
  cx -- read --mode smart src/lib.rs
```

If the helper times out, exits nonzero, writes invalid JSON, or returns an empty
summary, CX falls back to the local smart summary and reports the plugin failure
on stderr. The read command still returns useful file evidence.
