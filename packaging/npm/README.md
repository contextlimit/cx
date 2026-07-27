# @contextlimit/cx

This package installs the native `cx` binary for macOS or Linux on arm64 or
x64. CX is an independent local Rust CLI built for OpenAI Codex workflows. It
is not an OpenAI product.

```sh
npm install -g @contextlimit/cx
cx --version
```

The installer downloads the matching asset from the
[`contextlimit/cx` GitHub release](https://github.com/contextlimit/cx/releases),
verifies it against the release `checksums.txt`, marks it executable, and
atomically installs it inside this npm package.

The package does not:

- compile Rust;
- invoke a shell during installation;
- edit shell profiles;
- install into `~/.cx`;
- send telemetry.

The first command run through CX creates `~/.cx/db.sqlite` and records local
invocation and savings metrics by default. Full command text, source labels,
failure responses, and response previews remain opt-in.

Direct access to GitHub Releases is required. Environments that disable npm
lifecycle scripts should use Homebrew or the source installer instead.

Project documentation, Codex `AGENTS.md` instructions, command support, and the
local SQLite insights contract live in the
[source repository](https://github.com/contextlimit/cx).
