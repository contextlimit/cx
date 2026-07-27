# CX Homebrew Tap

The official CX Homebrew distribution uses the `contextlimit` tap:

```sh
brew install contextlimit/tap/cx
```

The tap repository is:

```text
https://github.com/contextlimit/homebrew-tap
```

The formula installs the same native binaries published for npm and direct
GitHub downloads. It does not compile Rust during installation.

Engineering revision tags such as `r137` are not Homebrew versions. Public
packages use semantic versions such as `v0.1.0`.

## Render The Formula

```sh
version=0.1.0

./packaging/homebrew/render-formula.sh \
  "$version" .tmp/release/checksums.txt \
  .tmp/homebrew-tap/Formula/cx.rb
```

The checksum manifest must contain exactly one SHA-256 row for each release
asset:

```text
cx-v0.1.0-darwin-arm64
cx-v0.1.0-darwin-x64
cx-v0.1.0-linux-arm64
cx-v0.1.0-linux-x64
```

The checked-in `Formula/cx.rb.in` is a template. Do not publish it while any
`@...@` placeholder remains.

For local proof only, pass a fourth base-URL argument and explicitly allow a
`file://` source:

```sh
CX_HOMEBREW_ALLOW_FILE_URL=1 \
  ./packaging/homebrew/render-formula.sh \
  "$version" .tmp/release/checksums.txt \
  .tmp/homebrew-tap/Formula/cx.rb \
  "file://$PWD/.tmp/release"
```

## Validate

From a checkout of `contextlimit/homebrew-tap`:

```sh
brew style Formula/cx.rb
brew audit --strict Formula/cx.rb
brew install ./Formula/cx.rb
brew test cx
brew uninstall cx
```

For tap CI:

```sh
brew test-bot --only-tap-syntax
brew test-bot --only-formulae
```

## Optional Automation

An automated release can update the tap after publication, but it requires a
fine-grained GitHub token with contents write access only to
`contextlimit/homebrew-tap`. Keep that token in the `contextlimit/cx` repository
Actions secrets and do not reuse a broad personal access token.
