# Security Policy

## Supported Versions

CX is currently pre-1.0. Security fixes target the latest release and the
current `main` branch. Older revision tags may not receive backports.

## Report A Vulnerability

Use [GitHub private vulnerability reporting](https://github.com/contextlimit/cx/security/advisories/new).
If that surface is unavailable, contact the maintainer privately through the
contact method listed on the
[contextlimit GitHub profile](https://github.com/contextlimit).

Do not post exploit details, real commands containing credentials, a real
`~/.cx/db.sqlite`, or raw failure artifacts in a public issue or community
channel.

Include:

- CX version and build revision from `cx --version`;
- operating system and architecture;
- the smallest synthetic command that reproduces the issue;
- expected native behavior and actual CX behavior;
- whether passthrough or insights were enabled;
- affected files, database rows, or artifact paths without secrets;
- impact and any known workaround.

We will acknowledge a report, reproduce it against the current release, and
coordinate disclosure after a fix is available.

## High-Sensitivity Areas

Security reports are especially valuable for:

- unintended shell execution or command injection;
- argv rewriting that changes command behavior;
- wrong exit codes or stderr loss;
- secret leakage through command text, reports, exports, process summaries, or
  failure artifacts;
- redaction bypasses;
- path traversal or unsafe capture and artifact paths;
- capture-file retention or permission issues;
- destructive insights migrations or installer behavior;
- evidence linked to the wrong invocation or report;
- recursive CX execution or routing-policy bypass;
- unsafe archive or SQLite input handling.

## Local Data

Insights are optional and disabled by default. CX has no vendor analytics
service or remote telemetry path. Failure artifacts and an enabled database can
contain sensitive command output, so protect them like development logs.
