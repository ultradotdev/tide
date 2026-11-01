#!/bin/sh
set -eu

PRODUCT="Tide"
RELEASE_BASE_DEFAULT="https://github.com/ultradotdev/tide/releases/download"
LATEST_RELEASE_URL_DEFAULT="https://github.com/ultradotdev/tide/releases/latest"
DEVELOPER_TEAM_ID="WN53UUCY94"
ATTESTATION_REPO="ultradotdev/tide"

die() {
  printf '%s\n' "tide installer: $*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Install or manage Tide binary releases.

Usage:
  install.sh [--version VERSION] [--add-to-path]
  install.sh --binary PATH [--prefix PATH]
  install.sh --rollback [VERSION]

Options:
  --binary PATH      Install a trusted locally built binary (no download/signing claim).
  --prefix PATH      Local install prefix, default ~/.local.
  --version VERSION  Install an exact Tide version.
  --rollback         Activate the retained previous release.
  --rollback VERSION Activate an already installed version.
  --add-to-path      Append the binary directory to the active shell startup file.
  -h, --help         Show this help.

Without --version, the latest published GitHub release is installed. GitHub
does not offer prereleases as the latest release; name one with --version.

Environment:
  TIDE_VERSION, TIDE_RELEASE_BASE_URL, TIDE_LATEST_VERSION_URL (a URL that
  redirects to .../releases/tag/vVERSION), TIDE_INSTALL_ROOT, TIDE_BIN_DIR,
  TIDE_PLATFORM, and TIDE_ARCH.
EOF
}

version="${TIDE_VERSION:-}"
rollback=0
rollback_version=""
add_to_path=0
binary=""
prefix=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --binary|--prefix)
      [ "$#" -ge 2 ] || die "$1 requires a value"
      if [ "$1" = --binary ]; then binary=$2; else prefix=$2; fi
      shift 2
      ;;
    --version)
      [ "$#" -ge 2 ] || die "--version requires a value"
      version=$2
      shift 2
      ;;
    --rollback)
      rollback=1
      if [ "$#" -ge 2 ] && [ "${2#-}" = "$2" ]; then
        rollback_version=$2
        shift 2
      else
        shift
      fi
      ;;
    --add-to-path)
      add_to_path=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) die "unknown option: $1" ;;
  esac
done

if [ -n "$binary" ]; then
  [ "$rollback" -eq 0 ] && [ -z "$version" ] || die '--binary cannot be combined with version/rollback'
  [ "$add_to_path" -eq 0 ] || die 'add PATH explicitly after local installation; --add-to-path is for downloaded releases'
  [ -f "$binary" ] && [ -x "$binary" ] && [ ! -L "$binary" ] || die '--binary requires an executable regular file'
  if [ -n "$prefix" ]; then
    case "$prefix" in /*) ;; *) die '--prefix must be absolute' ;; esac
    TIDE_INSTALL_ROOT="$prefix/lib/tide"
    TIDE_BIN_DIR="$prefix/bin"
    export TIDE_INSTALL_ROOT TIDE_BIN_DIR
  fi
  exec "$binary" --install-local
fi
[ -z "$prefix" ] || die '--prefix requires --binary'

case "${HOME:-}" in
  /*) ;;
  *) die "HOME must be an absolute path" ;;
esac

install_root="${TIDE_INSTALL_ROOT:-$HOME/.local/lib/tide}"
bin_dir="${TIDE_BIN_DIR:-$HOME/.local/bin}"
release_base="${TIDE_RELEASE_BASE_URL:-$RELEASE_BASE_DEFAULT}"
latest_release_url="${TIDE_LATEST_VERSION_URL:-$LATEST_RELEASE_URL_DEFAULT}"

case "$install_root" in
  /*/tide) ;;
  *) die "TIDE_INSTALL_ROOT must be an absolute path ending in /tide" ;;
esac
case "$bin_dir" in
  /*) ;;
  *) die "TIDE_BIN_DIR must be an absolute path" ;;
esac

validate_version() {
  case "$1" in
    ''|.|..|*[!0-9A-Za-z.-]*) die "invalid Tide version: $1" ;;
  esac
}

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    die "SHA-256 verification requires shasum or sha256sum"
  fi
}

download() {
  command -v curl >/dev/null 2>&1 || die "curl is required"
  case "$1" in
    https://*) ;;
    http://127.0.0.1:*|http://localhost:*) ;;
    *) die "downloads require HTTPS except for a loopback test server" ;;
  esac
  case "$1" in
    https://*) curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --max-filesize 536870912 --output "$2" "$1" ;;
    *) curl --fail --silent --show-error --proto '=http' --max-redirs 0 --max-filesize 536870912 --output "$2" "$1" ;;
  esac
}

# GitHub redirects /releases/latest to /releases/tag/vVERSION. Following the
# redirect avoids the rate-limited REST API; only the final URL is read.
latest_version() {
  command -v curl >/dev/null 2>&1 || die "curl is required"
  case "$1" in
    https://*) resolved=$(curl --fail --silent --show-error --head --location --proto '=https' --proto-redir '=https' --max-redirs 10 --output /dev/null --write-out '%{url_effective}' "$1") || resolved="" ;;
    http://127.0.0.1:*|http://localhost:*) resolved=$(curl --fail --silent --show-error --head --proto '=http' --output /dev/null --write-out '%{redirect_url}' "$1") || resolved="" ;;
    *) die "downloads require HTTPS except for a loopback test server" ;;
  esac
  resolved=${resolved%%[?#]*}
  case "$resolved" in
    */releases/tag/v?*) printf '%s\n' "${resolved##*/releases/tag/v}" ;;
    *) die "no published Tide release was found at $1 (GitHub's latest release excludes prereleases); pass --version VERSION" ;;
  esac
}

validate_installed_release() {
  release_dir=$1
  expected_version=$2
  [ -d "$release_dir" ] && [ ! -L "$release_dir" ] || die "installed release is missing or symlinked: $expected_version"
  [ -x "$release_dir/tide" ] || die "installed Tide binary is missing or not executable"
  [ -x "$release_dir/uninstall.sh" ] || die "installed uninstaller is missing or not executable"
  [ -f "$release_dir/VERSION.json" ] || die "installed VERSION.json is missing"
  grep -Eq '"product"[[:space:]]*:[[:space:]]*"Tide"' "$release_dir/VERSION.json" || die "installed release has the wrong product"
  case "$expected_version" in local-*) ;; *) grep -Eq '"version"[[:space:]]*:[[:space:]]*"'"$expected_version"'"' "$release_dir/VERSION.json" || die "installed release version mismatch" ;; esac
  for entry in tide uninstall.sh LICENSE ATTRIBUTIONS.md VERSION.json; do
    [ ! -L "$release_dir/$entry" ] || die "symlinked installed payload: $entry"
  done
  "$release_dir/tide" --verify-install-manifest "$release_dir" || die 'installed payload is invalid'
  case "$expected_version" in local-*) "$release_dir/tide" --version >/dev/null ;; *) "$release_dir/tide" --version | grep -F "v$expected_version" >/dev/null || die "installed Tide binary version mismatch" ;; esac
}

activate_release() {
  activate_version=$1
  validate_version "$activate_version"
  check=$install_root
  while [ "$check" != / ]; do
    [ ! -L "$check" ] || die 'refusing a symlinked installation directory'
    check=${check%/*}
    [ -n "$check" ] || check=/
  done
  [ ! -L "$bin_dir" ] || die 'refusing a symlinked binary directory'
  if [ -L "$bin_dir/tide" ]; then
    [ "$(readlink "$bin_dir/tide")" = "$install_root/current/tide" ] || die 'launcher belongs to another installation'
  elif [ -e "$bin_dir/tide" ]; then
    die 'refusing to replace a foreign executable'
  fi
  [ ! -L "$install_root/releases" ] || die 'refusing a symlinked release store'
  release_dir="$install_root/releases/$activate_version"
  validate_installed_release "$release_dir" "$activate_version"
  "$release_dir/tide" --activate-existing "$activate_version"
}

if [ "$rollback" -eq 1 ]; then
  [ -d "$install_root/releases" ] || die "no Tide releases are installed"
  mkdir -p "$bin_dir"
  if [ -n "$rollback_version" ]; then
    validate_version "$rollback_version"
    target_version=$rollback_version
  else
    [ -L "$install_root/previous" ] || die "no previous Tide release is retained"
    previous_target=$(readlink "$install_root/previous")
    case "$previous_target" in releases/*) target_version=${previous_target#releases/} ;; *) die "previous Tide link points outside the release store" ;; esac
  fi
  activate_release "$target_version"
  printf '%s\n' "Tide $target_version is now active."
  exit 0
fi

if [ -z "$version" ]; then
  version=$(latest_version "$latest_release_url") || exit 1
fi
validate_version "$version"

platform="${TIDE_PLATFORM:-}"
if [ -z "$platform" ]; then
  case "$(uname -s)" in Darwin) platform=darwin ;; Linux) platform=linux ;; *) die "unsupported operating system: $(uname -s)" ;; esac
fi
case "$platform" in darwin|linux) ;; *) die "unsupported Tide platform: $platform" ;; esac

architecture="${TIDE_ARCH:-}"
if [ -z "$architecture" ]; then
  case "$(uname -m)" in arm64|aarch64) architecture=arm64 ;; x86_64|amd64) architecture=x64 ;; *) die "unsupported CPU architecture: $(uname -m)" ;; esac
fi
case "$architecture" in arm64|x64) ;; *) die "unsupported Tide architecture: $architecture" ;; esac

case "$platform-$architecture" in
  darwin-arm64) extension=zip ;;
  darwin-x64) extension=zip ;;
  linux-arm64) extension=tar.gz ;;
  linux-x64) extension=tar.gz ;;
  *) die "unsupported Tide target: $platform-$architecture" ;;
esac

if [ -d "$install_root/releases/$version" ]; then
  activate_release "$version"
  printf '%s\n' "Tide $version is already installed and is now active."
  exit 0
fi

tag="v$version"
stem="tide-v$version-$platform-$architecture"
archive="$stem.$extension"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/tide-install.XXXXXX")
cleanup() { rm -rf "$temporary"; }
trap cleanup EXIT HUP INT TERM

download "$release_base/$tag/SHA256SUMS" "$temporary/SHA256SUMS"
download "$release_base/$tag/$archive" "$temporary/$archive"
expected_hash=$(awk -v file="$archive" '$2 == file { print $1 }' "$temporary/SHA256SUMS")
[ -n "$expected_hash" ] || die "$archive is absent from SHA256SUMS"
actual_hash=$(sha256_file "$temporary/$archive")
[ "$actual_hash" = "$expected_hash" ] || die "checksum mismatch for $archive"

# SHA256SUMS comes from the same host as the archive. Official releases also carry
# GitHub build provenance; check it whenever an authenticated GitHub CLI can.
if [ "$release_base" = "$RELEASE_BASE_DEFAULT" ] && command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  gh attestation verify "$temporary/$archive" --repo "$ATTESTATION_REPO" >/dev/null 2>&1 || die "GitHub build attestation verification failed for $archive"
elif [ "${TIDE_REQUIRE_ATTESTATION:-0}" = 1 ]; then
  die 'TIDE_REQUIRE_ATTESTATION=1 needs the official release host and an authenticated GitHub CLI (gh)'
fi

if [ "$platform" = darwin ]; then
  receipt="$stem.notarization.json"
  download "$release_base/$tag/$receipt" "$temporary/$receipt"
  expected_receipt_hash=$(awk -v file="$receipt" '$2 == file { print $1 }' "$temporary/SHA256SUMS")
  [ -n "$expected_receipt_hash" ] || die "$receipt is absent from SHA256SUMS"
  [ "$(sha256_file "$temporary/$receipt")" = "$expected_receipt_hash" ] || die "checksum mismatch for $receipt"
  grep -Eq '"notaryStatus"[[:space:]]*:[[:space:]]*"Accepted"' "$temporary/$receipt" || die "Apple notarization receipt is not accepted"
  grep -Eq '"artifactId"[[:space:]]*:[[:space:]]*"darwin-'"$architecture"'"' "$temporary/$receipt" || die "notarization receipt target does not match the archive"
  grep -Eq '"archiveSha256"[[:space:]]*:[[:space:]]*"'"$actual_hash"'"' "$temporary/$receipt" || die "notarization receipt does not match the archive"
fi

# Validate names, uniqueness and file types before an extractor may write them.
if [ "$extension" = zip ]; then
  command -v unzip >/dev/null 2>&1 || die 'unzip is required on macOS'
  unzip -Z -1 "$temporary/$archive" > "$temporary/members"
  unzip -Z -l "$temporary/$archive" > "$temporary/modes"
  awk '/^[-dlbcp?s][rwxstST?-]{9}/ { if (substr($0,1,1)!="-") exit 1; n++ } END { if(n!=5) exit 1 }' "$temporary/modes" || die 'archive contains unsafe file types'
else
  tar -tzf "$temporary/$archive" > "$temporary/members"
  tar -tvzf "$temporary/$archive" > "$temporary/modes"
  awk -v stem="$stem" 'substr($0,1,1)=="-" { n++; next } substr($0,1,1)=="d" && $NF==stem"/" { next } { exit 1 } END { if(n!=5) exit 1 }' "$temporary/modes" || die 'archive contains unsafe file types'
fi
awk -v stem="$stem" '
  BEGIN { expected[stem"/tide"]=1; expected[stem"/uninstall.sh"]=1; expected[stem"/VERSION.json"]=1; expected[stem"/LICENSE"]=1; expected[stem"/ATTRIBUTIONS.md"]=1 }
  $0==stem"/" { if(directory++) exit 1; next }
  { if(!expected[$0] || seen[$0]++) exit 1; n++ }
  END { if(n!=5) exit 1 }
' "$temporary/members" || die 'archive contains unexpected, duplicate or escaping paths'
mkdir "$temporary/extracted"
if [ "$extension" = zip ]; then
  (umask 022; ulimit -f 1048576; unzip -q "$temporary/$archive" -d "$temporary/extracted")
else
  (umask 022; ulimit -f 1048576; tar --no-same-owner --no-same-permissions -xzf "$temporary/$archive" -C "$temporary/extracted")
fi

payload="$temporary/extracted/$stem"
[ -d "$payload" ] || die "archive root does not match $stem"
[ -x "$payload/tide" ] || die "archive is missing executable tide"
[ -x "$payload/uninstall.sh" ] || die "archive is missing executable uninstall.sh"
[ -f "$payload/LICENSE" ] && [ -f "$payload/ATTRIBUTIONS.md" ] && [ -f "$payload/VERSION.json" ] || die "archive metadata is incomplete"
[ "$(find "$temporary/extracted" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" = 1 ] || die "archive contains unexpected top-level directories"

grep -Eq '"product"[[:space:]]*:[[:space:]]*"Tide"' "$payload/VERSION.json" || die "VERSION.json product mismatch"
grep -Eq '"version"[[:space:]]*:[[:space:]]*"'"$version"'"' "$payload/VERSION.json" || die "VERSION.json version mismatch"
grep -Eq '"platform"[[:space:]]*:[[:space:]]*"'"$platform"'"' "$payload/VERSION.json" || die "VERSION.json platform mismatch"
grep -Eq '"architecture"[[:space:]]*:[[:space:]]*"'"$architecture"'"' "$payload/VERSION.json" || die "VERSION.json architecture mismatch"

if [ "$platform" = darwin ]; then
  command -v codesign >/dev/null 2>&1 || die "codesign is required on macOS"
  # A receipt is provenance, not an Apple trust decision. Verify the executable
  # against Apple's certificate chain, our team and its online notarization ticket.
  codesign --verify --strict --verbose=2 --check-notarization \
    -R="notarized and anchor apple generic and certificate leaf[subject.OU] = \"$DEVELOPER_TEAM_ID\"" \
    "$payload/tide" >/dev/null 2>&1 || die "Tide Apple signature or notarization verification failed"
  codesign -d --verbose=4 "$payload/tide" 2> "$temporary/codesign.txt" || die "Tide signing metadata is unavailable"
  grep -F "TeamIdentifier=$DEVELOPER_TEAM_ID" "$temporary/codesign.txt" >/dev/null || die "Tide is not signed by Ultra.dev's release team"
fi

"$payload/tide" --version | grep -F "v$version" >/dev/null || die "Tide binary version mismatch"
"$payload/tide" --check-terminal >/dev/null || die "Tide terminal runtime check failed"

"$payload/tide" --verify-install-manifest "$payload" || die 'payload verification failed'
"$payload/tide" --check-catalogs >/dev/null || die 'Tide catalog check failed'
"$payload/tide" --install-release "$payload"

if [ "$add_to_path" -eq 1 ]; then
  path_line="export PATH=\"$bin_dir:\$PATH\""
  case "${SHELL:-}" in */zsh) shell_file="$HOME/.zshrc" ;; */bash) shell_file="$HOME/.bashrc" ;; *) die "--add-to-path supports zsh and bash; add $bin_dir manually" ;; esac
  if ! grep -F "$path_line" "$shell_file" >/dev/null 2>&1; then printf '\n%s\n' "$path_line" >> "$shell_file"; fi
fi

printf '%s\n' "Installed $PRODUCT $version at $install_root/releases/$version"
case ":${PATH:-}:" in
  *:"$bin_dir":*) ;;
  *)
    printf '%s\n' "$bin_dir is not on PATH. Add this line to your shell startup file:"
    printf '  export PATH="%s:$PATH"\n' "$bin_dir"
    ;;
esac
