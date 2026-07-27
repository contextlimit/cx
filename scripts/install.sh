#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)

if [ "${CX_INSTALL_SKIP_BUILD:-0}" != "1" ]; then
  cargo build --release --bin cx --manifest-path "$repo_root/Cargo.toml"
fi

bin_path=${CX_INSTALL_BIN_PATH:-"$repo_root/target/release/cx"}
install_dir=${HOME}/.local/bin
install_path=${install_dir}/cx
runtime_dir=${HOME}/.cx/bin
runtime_path=${runtime_dir}/cx

if [ ! -x "$bin_path" ]; then
  echo "cx install: expected executable at $bin_path" >&2
  exit 1
fi

mkdir -p "$install_dir" "$runtime_dir" "${HOME}/.config/cx" "${HOME}/.cx/cache"
runtime_tmp=${runtime_path}.tmp.$$
install_tmp=${install_path}.tmp.$$
cleanup_install_temps() {
  rm -f "$runtime_tmp" "$install_tmp"
}
trap cleanup_install_temps EXIT
trap 'exit 1' HUP INT TERM

cp "$bin_path" "$runtime_tmp"
chmod 755 "$runtime_tmp"
mv -f "$runtime_tmp" "$runtime_path"

cat > "$install_tmp" <<EOF
#!/usr/bin/env sh
if [ ! -x "$runtime_path" ]; then
  echo "cx wrapper: missing runtime binary at $runtime_path" >&2
  exit 127
fi
exec "$runtime_path" "\$@"
EOF
chmod 755 "$install_tmp"
mv -f "$install_tmp" "$install_path"

echo "installed $install_path -> $runtime_path"

case ":${PATH:-}:" in
  *":$install_dir:"*) ;;
  *)
    echo "warning: $install_dir is not on PATH" >&2
    ;;
esac
