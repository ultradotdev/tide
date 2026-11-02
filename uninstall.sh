#!/bin/sh
set -eu

die() { printf '%s\n' "tide uninstaller: $*" >&2; exit 1; }
purge=0
yes=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --purge) purge=1 ;;
    --yes) yes=1 ;;
    -h|--help) printf '%s\n' 'Usage: tide --uninstall [--purge [--yes]]' 'Remove a verified managed installation. User data is kept unless --purge is confirmed.'; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
  shift
done
# Run outside an installed release (`curl ... | sh`, or a source checkout's
# copy), hand over to the active release's own uninstaller, which verifies
# ownership before it removes anything.
hand_off() {
  case "${HOME:-}" in /*) ;; *) die 'HOME must be absolute' ;; esac
  managed=${TIDE_INSTALL_ROOT:-$HOME/.local/lib/tide}
  case "$managed" in /*/tide) ;; *) die 'TIDE_INSTALL_ROOT must be an absolute path ending in /tide' ;; esac
  [ -f "$managed/current/uninstall.sh" ] || die "no managed Tide installation was found at $managed; if Tide was installed with Homebrew, run: brew uninstall tide"
  set --
  [ "$purge" -eq 0 ] || set -- "$@" --purge
  [ "$yes" -eq 0 ] || set -- "$@" --yes
  exec /bin/sh "$managed/current/uninstall.sh" "$@"
}
case "$0" in
  */uninstall.sh) ;;
  *) hand_off ;;
esac
[ ! -L "$0" ] || die 'refusing a symlinked uninstaller'
release=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
releases=${release%/*}
[ "${releases##*/}" = releases ] || hand_off
identity=${release##*/}
case "$identity" in ''|.|..|*[!0-9A-Za-z.-]*) die 'invalid release identity' ;; esac
root=${releases%/*}
[ "${root##*/}" = tide ] || die 'invalid Tide installation root'
[ ! -L "$root" ] && [ ! -L "$root/releases" ] || die 'refusing a symlinked store'
[ -f "$release/VERSION.json" ] && [ ! -L "$release/VERSION.json" ] || die 'missing release manifest'
[ -L "$root/current" ] && [ "$(readlink "$root/current")" = "releases/$identity" ] || die 'only the active installed release may uninstall Tide'
case "${HOME:-}" in /*) ;; *) die 'HOME must be absolute' ;; esac
[ "$root" != "$HOME" ] || die 'unsafe installation root'
if [ -f "$root/BIN_DIR" ] && [ ! -L "$root/BIN_DIR" ]; then
  bin_dir=$(cat "$root/BIN_DIR")
else
  bin_dir=${TIDE_BIN_DIR:-$HOME/.local/bin}
fi
case "$bin_dir" in /*) ;; *) die 'invalid binary directory' ;; esac
[ ! -L "$bin_dir" ] || die 'refusing a symlinked binary directory'
launcher=$bin_dir/tide
[ -L "$launcher" ] && [ "$(readlink "$launcher")" = "$root/current/tide" ] || die 'launcher does not belong to this installation'
[ ! -e "$root/install.lock" ] && [ ! -L "$root/install.lock" ] || die 'installation is busy; inspect install.lock before retrying'

# Confirm and validate every purge destination before changing the installation.
if [ "$purge" -eq 1 ]; then
  if [ "$yes" -ne 1 ]; then
    [ -t 0 ] || die '--purge requires --yes in noninteractive use'
    printf '%s' 'Remove Tide settings, sessions, credentials, history and cache? [y/N] ' >&2
    read -r answer
    case "$answer" in y|Y|yes|YES) ;; *) die 'purge cancelled' ;; esac
  fi
  for path in "${XDG_CONFIG_HOME:-$HOME/.config}/tide" "${XDG_DATA_HOME:-$HOME/.local/share}/tide" "${XDG_STATE_HOME:-$HOME/.local/state}/tide" "${XDG_CACHE_HOME:-$HOME/.cache}/tide"; do
    case "$path" in /*/tide) ;; *) die "unsafe purge path: $path" ;; esac
    case "/$path/" in */../*|*/./*) die "unsafe purge path: $path" ;; esac
    check=$path
    while [ "$check" != / ]; do
      [ ! -L "$check" ] || die "refusing symlinked purge path: $path"
      check=${check%/*}
      [ -n "$check" ] || check=/
    done
  done
fi
mkdir "$root/install.lock" || die 'installation became busy'
rm -- "$launcher"
rm -rf -- "$root"
if [ "$purge" -eq 1 ]; then
  for path in "${XDG_CONFIG_HOME:-$HOME/.config}/tide" "${XDG_DATA_HOME:-$HOME/.local/share}/tide" "${XDG_STATE_HOME:-$HOME/.local/state}/tide" "${XDG_CACHE_HOME:-$HOME/.cache}/tide"; do
    rm -rf -- "$path"
  done
fi
printf '%s\n' 'Tide uninstalled.'
[ "$purge" -eq 1 ] || printf '%s\n' 'Settings, sessions, credentials and history were kept.'
