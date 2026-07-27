#!/usr/bin/env sh
set -eu

fail() {
  echo "cx install: $*" >&2
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail "required command not found: $1"
  fi
}

download() {
  source_url=$1
  destination=$2

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 --retry-delay 1 "$source_url" -o "$destination" </dev/null
    return
  fi
  if command -v wget >/dev/null 2>&1; then
    wget -q "$source_url" -O "$destination" </dev/null
    return
  fi
  fail "curl or wget is required"
}

sha256_file() {
  target=$1

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$target" | awk '{print $1}'
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$target" | awk '{print $1}'
    return
  fi
  fail "sha256sum or shasum is required"
}

validate_version() {
  if ! awk -v version="$1" \
    'BEGIN { exit(version ~ /^[0-9]+\.[0-9]+\.[0-9]+$/ ? 0 : 1) }'
  then
    fail "CX_INSTALL_VERSION must be a stable semantic version such as 0.1.1"
  fi
}

if [ -z "${HOME:-}" ]; then
  fail "HOME is required"
fi

method=${CX_INSTALL_METHOD:-binary}
case "$method" in
  binary | source) ;;
  *) fail "CX_INSTALL_METHOD must be binary or source" ;;
esac

release_root=${CX_INSTALL_RELEASE_ROOT:-https://github.com/contextlimit/cx/releases}
repository_url=${CX_INSTALL_REPOSITORY_URL:-https://github.com/contextlimit/cx.git}
allow_file_url=${CX_INSTALL_ALLOW_FILE_URL:-0}

case "$release_root" in
  https://*) ;;
  file://*)
    [ "$allow_file_url" = "1" ] ||
      fail "file release URLs require CX_INSTALL_ALLOW_FILE_URL=1"
    ;;
  *) fail "CX_INSTALL_RELEASE_ROOT must use https" ;;
esac
release_root=${release_root%/}

case "$repository_url" in
  https://*) ;;
  file://*)
    [ "$allow_file_url" = "1" ] ||
      fail "file repository URLs require CX_INSTALL_ALLOW_FILE_URL=1"
    ;;
  *) fail "CX_INSTALL_REPOSITORY_URL must use https" ;;
esac

case "$(uname -s)" in
  Darwin) operating_system=darwin ;;
  Linux) operating_system=linux ;;
  *) fail "unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
  arm64 | aarch64) architecture=arm64 ;;
  x86_64 | amd64) architecture=x64 ;;
  *) fail "unsupported architecture: $(uname -m)" ;;
esac

platform=${operating_system}-${architecture}
requested_version=${CX_INSTALL_VERSION:-}
if [ -n "$requested_version" ]; then
  validate_version "$requested_version"
  release_base=${release_root}/download/v${requested_version}
else
  release_base=${release_root}/latest/download
fi

temp_root=${TMPDIR:-/tmp}
mkdir -p "$temp_root"
temp_dir=$(mktemp -d "${temp_root%/}/cx-install.XXXXXX")
runtime_tmp=
wrapper_tmp=
cleanup() {
  rm -rf "$temp_dir"
  if [ -n "$runtime_tmp" ]; then
    rm -f "$runtime_tmp"
  fi
  if [ -n "$wrapper_tmp" ]; then
    rm -f "$wrapper_tmp"
  fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

manifest=${temp_dir}/checksums.txt
download "${release_base}/checksums.txt" "$manifest"

if [ -n "$requested_version" ]; then
  version=$requested_version
  asset=cx-v${version}-${platform}
  manifest_row=$(
    awk -v asset="$asset" \
      '$2 == asset { count += 1; checksum = $1 }
       END {
         if (count == 1) {
           print checksum, asset
         } else {
           exit 1
         }
       }' \
      "$manifest"
  ) || fail "release checksum manifest does not contain exactly one $asset entry"
else
  manifest_row=$(
    awk -v suffix="-${platform}" \
      '$2 ~ /^cx-v[0-9]+\.[0-9]+\.[0-9]+-/ &&
       substr($2, length($2) - length(suffix) + 1) == suffix {
         count += 1
         checksum = $1
         asset = $2
       }
       END {
         if (count == 1) {
           print checksum, asset
         } else {
           exit 1
         }
       }' \
      "$manifest"
  ) || fail "latest checksum manifest does not identify exactly one ${platform} asset"
  asset=${manifest_row#* }
  version=${asset#cx-v}
  version=${version%-"${platform}"}
  validate_version "$version"
fi

checksum=${manifest_row%% *}
case "$checksum" in
  "" | *[!0-9a-f]*) fail "release checksum must use lowercase hexadecimal" ;;
esac
if [ "${#checksum}" -ne 64 ]; then
  fail "release checksum must contain 64 hexadecimal characters"
fi

case "$method" in
  binary)
    source_binary=${temp_dir}/${asset}
    download "${release_base}/${asset}" "$source_binary"
    actual_checksum=$(sha256_file "$source_binary")
    if [ "$actual_checksum" != "$checksum" ]; then
      fail "checksum mismatch for $asset"
    fi
    chmod 755 "$source_binary"
    ;;
  source)
    require_command git
    require_command cargo
    source_root=${temp_dir}/source
    git clone --depth 1 --branch "v${version}" --single-branch \
      "$repository_url" "$source_root" </dev/null
    cargo build --release --locked --bin cx \
      --manifest-path "$source_root/Cargo.toml" </dev/null
    source_binary=${source_root}/target/release/cx
    ;;
esac

if [ ! -x "$source_binary" ]; then
  fail "expected an executable CX binary at $source_binary"
fi

reported_version=$("$source_binary" --version </dev/null)
case "$reported_version" in
  "cx ${version} ("*")") ;;
  *)
    fail "downloaded binary reported '$reported_version', expected cx ${version}"
    ;;
esac

install_dir=${HOME}/.local/bin
runtime_dir=${HOME}/.cx/bin
runtime_path=${runtime_dir}/cx
wrapper_path=${install_dir}/cx

mkdir -p "$install_dir" "$runtime_dir" "${HOME}/.config/cx" "${HOME}/.cx/cache"
runtime_tmp=${runtime_path}.tmp.$$
wrapper_tmp=${wrapper_path}.tmp.$$

cp "$source_binary" "$runtime_tmp"
chmod 755 "$runtime_tmp"
mv -f "$runtime_tmp" "$runtime_path"
runtime_tmp=

cat >"$wrapper_tmp" <<EOF
#!/usr/bin/env sh
if [ ! -x "$runtime_path" ]; then
  echo "cx wrapper: missing runtime binary at $runtime_path" >&2
  exit 127
fi
exec "$runtime_path" "\$@"
EOF
chmod 755 "$wrapper_tmp"
mv -f "$wrapper_tmp" "$wrapper_path"
wrapper_tmp=

installed_version=$("$wrapper_path" --version </dev/null)
case "$installed_version" in
  "cx ${version} ("*")") ;;
  *) fail "installed wrapper did not report cx ${version}" ;;
esac

echo "installed $installed_version"
echo "runtime: $runtime_path"
echo "command: $wrapper_path"

case ":${PATH:-}:" in
  *":$install_dir:"*) ;;
  *)
    echo "warning: $install_dir is not on PATH" >&2
    echo "add this line to your shell profile:" >&2
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\"" >&2
    ;;
esac
