#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
command -v cargo >/dev/null 2>&1 || { printf '%s\n' 'Rust/Cargo is required to build Tide from source.' >&2; exit 1; }
cd "$root"
cargo build --locked --release -p tide
case "${1:-}" in
  --build-only) [ "$#" -eq 1 ] || exit 64; printf '%s\n' "Built $root/target/release/tide" ;;
  *) exec /bin/sh "$root/install.sh" --binary "$root/target/release/tide" "$@" ;;
esac
