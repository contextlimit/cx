#!/usr/bin/env sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname "$0")" && pwd)

node "${script_dir}/build-installer.mjs"
exec npx --yes wrangler@4.114.0 deploy \
  --config "${script_dir}/wrangler.json" \
  "$@"
