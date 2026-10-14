#!/bin/sh
#
# Centinel installer. Either way round:
#
#   curl --proto '=https' --tlsv1.2 -sSf \
#       https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh | sh
#
#   git clone https://github.com/bennyhodl/centinel && cd centinel && ./install.sh
#
# Piped, the sources come from git. Run from a clone, they come from the clone — so a
# contributor testing a change installs the change, not master. The check is whether this
# script is a file on disk beside a workspace, because `$0` is the shell when it is piped.
#
# There are two ways to end up with the binary, and this script picks one:
#
#   download   a release asset, when a release carries one this host can run
#   build      from source, which is every other host and the fallback for every failure
#
# A release carries one asset per platform, and none of them carries a GPU backend:
# Centinel runs no model itself. Embedding, reranking and transcription are requests to the
# servers `[inference]` names in the config file, and the GPU is theirs.
#
#   aarch64-apple-darwin
#   x86_64-unknown-linux-gnu    x86-64-v3, so AVX2 — any x86_64 since about 2013
#   aarch64-unknown-linux-gnu
#
# Nothing about the download is load-bearing. No asset for this host, no release yet, a
# checksum that does not match, a binary that will not start — each falls back to the build
# this script did before it could download anything, so the worst a broken release does is
# cost one HTTP request. That is also why the download is tried before Rust is checked for:
# a host that downloads does not need a toolchain at all.
#
# Nothing is prompted, so an unattended run behaves the same as an attended one. Every
# choice is a flag or an environment variable.

set -eu

# The workspace `rust-version`. LanceDB sets it; see the root Cargo.toml.
MSRV="1.91"

PKG_MAIN="centinel"

# What releases before the inference servers installed beside `centinel`, and where they
# kept model weights. Neither is used any more; the install says so and leaves the
# deleting to the operator.
OLD_WORKER="centinel-whisper"

REPO="https://github.com/bennyhodl/centinel"

# Release asset names. No version in the name on purpose: it makes
# `releases/latest/download/<name>` a fixed URL, so updating is the same command as
# installing rather than a second thing to remember.
ASSET_MACOS_ARM64="centinel-aarch64-apple-darwin.tar.gz"
ASSET_LINUX_X86_64="centinel-x86_64-unknown-linux-gnu.tar.gz"
ASSET_LINUX_ARM64="centinel-aarch64-unknown-linux-gnu.tar.gz"

# A cold build unpacks and compiles arrow, datafusion and lance. Below this it is close
# enough that "no space left on device" is a real way to lose twenty minutes, and cargo
# does not check first.
DISK_WANT_GB=10

# The clone this script was run from, not the working directory — `../centinel/install.sh`
# has to install the same thing `./install.sh` does. Empty when there is no clone, which
# is the piped case: `$0` is then the shell, so there is no path to take a dirname of.
SRC=""
if [ -f "$0" ]; then
    _dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    if [ -f "$_dir/crates/$PKG_MAIN/Cargo.toml" ]; then SRC=$_dir; fi
fi

# ---------------------------------------------------------------- output

if [ -t 2 ]; then
    B=$(printf '\033[1m') R=$(printf '\033[31m') Y=$(printf '\033[33m')
    G=$(printf '\033[32m') D=$(printf '\033[2m')  N=$(printf '\033[0m')
else
    B='' R='' Y='' G='' D='' N=''
fi

say()  { printf '%s\n' "$*" >&2; }
step() { printf '%s==>%s %s\n' "$B" "$N" "$*" >&2; }
note() { printf '    %s%s%s\n' "$D" "$*" "$N" >&2; }
warn() { printf '%s warn%s %s\n' "$Y" "$N" "$*" >&2; }
die()  { printf '%serror%s %s\n' "$R" "$N" "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

# $1 >= $2 as dotted numeric versions, compared field by field so 1.100 beats 1.9.
version_ge() {
    if [ "$1" = "$2" ]; then return 0; fi
    awk -v a="$1" -v b="$2" 'BEGIN {
        na = split(a, x, "."); nb = split(b, y, ".")
        n = na > nb ? na : nb
        for (i = 1; i <= n; i++) {
            u = (i <= na ? x[i] + 0 : 0); v = (i <= nb ? y[i] + 0 : 0)
            if (u > v) exit 0
            if (u < v) exit 1
        }
        exit 0
    }'
}

usage() {
    cat >&2 <<EOF
${B}Centinel installer${N} — downloads a release binary when this host can run one and
builds from source when it cannot.

  curl --proto '=https' --tlsv1.2 -sSf $REPO/raw/master/install.sh | sh
  curl ... | sh -s -- --deps      ${D}flags, through the pipe${N}
  ./install.sh --deps             ${D}from a clone${N}

Re-run the same command to update. It reads the version off the latest release and stops
early when that is the one already installed.

${B}Options${N}
  --build            Build from source, even where a release binary would fit. This is
                     already what a clone does — it installs the clone.
  --download         Download a release binary, and fail rather than quietly building
                     when this host has no asset. Works from a clone too.
  --force            Install again even when the release is the version already here.
  --tag <tag>        A released tag rather than the latest release or the default branch.
  --bin-dir <dir>    Where the binary goes. A source build must end in \`bin\` —
                     cargo owns the layout under its install root.
  --portable         Build for the baseline CPU of this architecture instead of this
                     host's. Slower — it is what makes the binary copyable to another
                     machine. Default is to tune for the CPU that is building. Downloaded
                     binaries are built this way already.
  --deps             Also install what is missing, using the package manager found on
                     this host: ffmpeg and yt-dlp, and the toolchain when building.
                     Without it they are only reported.
  --no-doctor        Skip the closing \`centinel doctor\` run.
  -h, --help         This text.

${B}Environment${N}
  CENTINEL_BIN_DIR, CENTINEL_TAG — same as the flags above.
  CENTINEL_METHOD=auto|download|build — same as --download and --build.
  CENTINEL_NATIVE=0 — same as --portable.

${B}What it does not do${N}
  Install Rust. A download does not need it. If a build does and \`cargo\` is missing, it
  prints the one command that fixes that and stops.

  Run a model. Embedding, reranking and transcription go to the servers \`[inference]\`
  names in centinel.toml — llama-server, whisper.cpp's server, or anything that speaks
  the OpenAI API. \`centinel doctor\` says which are answering.
EOF
}

# ---------------------------------------------------------------- arguments

BIN_DIR="${CENTINEL_BIN_DIR:-}"
METHOD="${CENTINEL_METHOD:-auto}"
NATIVE="${CENTINEL_NATIVE:-1}"
TAG="${CENTINEL_TAG:-}"
FORCE=0
WITH_DEPS=0
RUN_DOCTOR=1

while [ $# -gt 0 ]; do
    case "$1" in
        # Gone with the in-process models. Named rather than "unknown", because the
        # person typing it has a GPU and needs to know where it went.
        --accel)   die "--accel is gone: Centinel runs no model itself. Give the GPU to the
      server [inference] names — llama-server, whisper.cpp's server — and run this without it." ;;
        --bin-dir) [ $# -ge 2 ] || die "--bin-dir needs a value"; BIN_DIR=$2; shift 2 ;;
        --tag)     [ $# -ge 2 ] || die "--tag needs a value"; TAG=$2; shift 2 ;;
        --build)     METHOD=build; shift ;;
        --download)  METHOD=download; shift ;;
        --force)     FORCE=1; shift ;;
        --portable)  NATIVE=0; shift ;;
        --deps)      WITH_DEPS=1; shift ;;
        --no-doctor) RUN_DOCTOR=0; shift ;;
        -h|--help)   usage; exit 0 ;;
        *) usage; die "unknown option: $1" ;;
    esac
done

case "$METHOD" in
    auto|download|build) ;;
    *) die "CENTINEL_METHOD is auto, download or build — not \`$METHOD\`" ;;
esac

# ---------------------------------------------------------------- the host

case "$(uname -s)" in
    Darwin) OS=macos ;;
    Linux)  OS=linux ;;
    MINGW*|MSYS*|CYGWIN*)
        die "this script needs a POSIX shell. On Windows, run the \`cargo install\` command
      from the README by hand." ;;
    *) die "unsupported system: $(uname -s)" ;;
esac

ARCH=$(uname -m)
case "$ARCH" in
    arm64|aarch64) ARCH=arm64 ;;
    x86_64|amd64)  ARCH=x86_64 ;;
    *) warn "untested architecture $ARCH — the build decides whether it works" ;;
esac

# ---------------------------------------------------------------- package manager

# Only ever names a manager that is actually installed here. `doctor` leaves the fix for
# ffmpeg and yt-dlp empty on purpose — a guessed `brew install` is wrong on most machines —
# and a detected manager is not a guess.
PM=""
PM_INSTALL=""
if   have brew;    then PM=brew;   PM_INSTALL="brew install"
elif have apt-get; then PM=apt;    PM_INSTALL="sudo apt-get install -y"
elif have dnf;     then PM=dnf;    PM_INSTALL="sudo dnf install -y"
elif have pacman;  then PM=pacman; PM_INSTALL="sudo pacman -S --noconfirm"
elif have zypper;  then PM=zypper; PM_INSTALL="sudo zypper install -y"
elif have apk;     then PM=apk;    PM_INSTALL="sudo apk add"
fi

# What each gap is called in each manager. Empty means the manager has no such package:
# on macOS the compiler comes from the Xcode command line tools, which is a different
# command and is checked separately.
pkg_for() {
    case "$PM:$1" in
        brew:protoc)             echo protobuf ;;
        brew:node)               echo node ;;
        apt:cc)                  echo build-essential ;;
        apt:protoc)              echo "protobuf-compiler libprotobuf-dev" ;;
        apt:node)                echo "nodejs npm" ;;
        dnf:cc)                  echo "gcc make" ;;
        dnf:protoc)              echo "protobuf-compiler protobuf-devel" ;;
        dnf:node)                echo "nodejs npm" ;;
        pacman:cc)               echo base-devel ;;
        pacman:protoc)           echo protobuf ;;
        pacman:node)             echo "nodejs npm" ;;
        zypper:cc)               echo "gcc make" ;;
        zypper:protoc)           echo "protobuf-devel" ;;
        zypper:node)             echo "nodejs npm" ;;
        apk:cc)                  echo build-base ;;
        apk:protoc)              echo protobuf-dev ;;
        apk:node)                echo "nodejs npm" ;;
    esac
}

# Why each one is needed, in the report. A person who is about to install packages on
# somebody's word deserves to know which dependency asked for them.
why_for() {
    case "$1" in
        cc)       echo "a C compiler, for SQLite, zstd and lz4" ;;
        protoc)   echo "six lance crates run prost-build, and none of them vendor it" ;;
        node)     echo "builds and runs the authenticated TanStack Start web server" ;;
    esac
}

PKGS=""
add_pkgs() {
    for _p in $1; do
        case " $PKGS " in *" $_p "*) ;; *) PKGS="$PKGS $_p" ;; esac
    done
}

# ---------------------------------------------------------------- destination

# Where a previous install put it, when there was one. A download that lands somewhere
# else does not replace the old binary, it shadows it — and then PATH order decides
# which version runs, which is a bad way to find out an update did nothing.
if [ -z "$BIN_DIR" ]; then
    _cargo_bin="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin"
    if have cargo || [ -x "$_cargo_bin/$PKG_MAIN" ]; then
        BIN_DIR=$_cargo_bin
    else
        # No Rust on this host, and none needed — a download does not want one.
        BIN_DIR="$HOME/.local/bin"
    fi
fi

# ---------------------------------------------------------------- what a download offers

cpu_has() { grep -qw "$1" /proc/cpuinfo 2>/dev/null; }

# The asset for this host, or empty with a reason on stderr. Empty is not a failure: it
# is the answer "build", which is what this script did for every host before releases
# carried binaries at all.
asset_for_host() {
    case "$OS/$ARCH" in
        macos/arm64)
            printf '%s' "$ASSET_MACOS_ARM64" ;;
        linux/x86_64)
            # Built for x86-64-v3, which is AVX2 and FMA — any x86_64 since about 2013.
            # Older than that and the binary faults on an instruction rather than saying
            # anything useful, so it is not offered.
            if ! cpu_has avx2; then
                note "this CPU has no AVX2, and the release binary is built for it"
                return 0
            fi
            printf '%s' "$ASSET_LINUX_X86_64" ;;
        linux/arm64)
            printf '%s' "$ASSET_LINUX_ARM64" ;;
        *)
            note "no release binary for $OS/$ARCH" ;;
    esac
}

# ---------------------------------------------------------------- fetching

TMP=""
cleanup() { if [ -n "$TMP" ]; then rm -rf "$TMP"; fi; }
trap cleanup EXIT INT TERM

# Quiet, including on failure: every caller reports the URL it wanted and carries on to a
# build, so curl's own `error 404` above that line is one message too many.
fetch_to() {
    if have curl; then
        curl --proto '=https' --tlsv1.2 -sSfL "$1" -o "$2" 2>/dev/null
    elif have wget; then
        wget -q -O "$2" "$1"
    else
        return 1
    fi
}

# The tag `releases/latest` redirects to. Empty when it cannot be worked out — which only
# costs the "already installed" shortcut, not the download. A private repository answers
# 404 to an anonymous request, so on one of those this is always empty and every install
# is a build.
latest_tag() {
    if have curl; then
        _url=$(curl --proto '=https' --tlsv1.2 -sSL -o /dev/null \
                    -w '%{url_effective}' "$REPO/releases/latest" 2>/dev/null) || return 0
        case "$_url" in
            */releases/tag/*) printf '%s' "${_url##*/}" ;;
        esac
    fi
}

installed_version() {
    if [ -x "$BIN_DIR/$PKG_MAIN" ]; then
        "$BIN_DIR/$PKG_MAIN" --version 2>/dev/null | awk '{print $2}'
    fi
}

# 0 installed, 1 could not — and 1 always means "build instead", never "stop".
try_download() {
    _asset=$1
    _want=$TAG
    if [ -z "$_want" ]; then _want=$(latest_tag); fi

    if [ -n "$_want" ] && [ "$FORCE" = 0 ]; then
        _here=$(installed_version)
        if [ -n "$_here" ] && [ "v$_here" = "$_want" ]; then
            say ""
            step "already at $_want"
            note "in $BIN_DIR. --force installs it again."
            say ""
            exit 0
        fi
    fi

    if [ -n "$TAG" ]; then
        _base="$REPO/releases/download/$TAG"
    else
        _base="$REPO/releases/latest/download"
    fi

    say ""
    step "downloading ${_want:-the latest release}"
    note "asset        $_asset"
    note "into         $BIN_DIR"

    TMP=$(mktemp -d 2>/dev/null || mktemp -d -t centinel) || return 1

    if ! fetch_to "$_base/$_asset" "$TMP/$_asset"; then
        warn "no release binary at $_base/$_asset"
        return 1
    fi

    # A published checksum that does not match is the one download failure that is not
    # allowed to fall back quietly to a build. Everything else here is a shrug; this is
    # somebody's release being wrong, or somebody's network handing over another file.
    if fetch_to "$_base/$_asset.sha256" "$TMP/$_asset.sha256"; then
        if have sha256sum; then
            (cd "$TMP" && sha256sum -c "$_asset.sha256" >/dev/null 2>&1) \
                || die "the download does not match its published sha256. Nothing was
      installed. Try again, and if it happens twice say so — that is worth knowing about."
        elif have shasum; then
            (cd "$TMP" && shasum -a 256 -c "$_asset.sha256" >/dev/null 2>&1) \
                || die "the download does not match its published sha256. Nothing was
      installed. Try again, and if it happens twice say so — that is worth knowing about."
        else
            warn "no sha256sum or shasum here, so the download was not verified"
        fi
    else
        warn "no published checksum for $_asset, so the download was not verified"
    fi

    if ! tar -xzf "$TMP/$_asset" -C "$TMP"; then
        warn "the release asset did not unpack"
        return 1
    fi

    if [ ! -f "$TMP/$PKG_MAIN" ]; then
        warn "$_asset does not contain $PKG_MAIN"
        return 1
    fi
    chmod +x "$TMP/$PKG_MAIN"

    # curl sets no quarantine attribute, but a browser download does, and clearing one
    # that is not there costs nothing.
    if [ "$OS" = macos ] && have xattr; then
        xattr -d com.apple.quarantine "$TMP/$PKG_MAIN" 2>/dev/null || true
    fi

    # The check that catches what the checks above cannot: a glibc too old, a macOS older
    # than the one that built it. The process starts, the dynamic linker resolves
    # everything, and it prints a version — and if it does not, a build is still on the
    # table and nothing has been written yet.
    if ! "$TMP/$PKG_MAIN" --version >/dev/null 2>&1; then
        warn "the release binary does not run on this host"
        return 1
    fi

    mkdir -p "$BIN_DIR" || return 1

    # Into place by rename, which is atomic within a directory and, unlike writing over
    # the file, does not fail on a binary that is running right now.
    cp "$TMP/$PKG_MAIN" "$BIN_DIR/.$PKG_MAIN.new" || return 1
    chmod 755 "$BIN_DIR/.$PKG_MAIN.new"
    mv "$BIN_DIR/.$PKG_MAIN.new" "$BIN_DIR/$PKG_MAIN" || return 1

    return 0
}

# ---------------------------------------------------------------- source build

check_rust() {
    have cargo && have rustc || die "no Rust toolchain, and this host is building from
      source. Install one, then re-run this:

      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"

    rustc_version=$(rustc --version | awk '{print $2}' | cut -d- -f1)
    version_ge "$rustc_version" "$MSRV" || die "Rust $rustc_version is too old — Centinel
      needs $MSRV or newer. \`rustup update stable\` fixes it."
    note "rust $rustc_version"
}

# protoc, and the well-known types it resolves imports against — two packages on Debian and
# only the first is obvious. `protobuf-compiler` is the binary; `google/protobuf/empty.proto`
# and the rest live in `libprotobuf-dev`, and lance imports it. With the compiler alone,
# protoc runs and then fails on lance's own import.
have_protoc() {
    if [ -n "${PROTOC:-}" ]; then return 0; fi
    if ! have protoc; then return 1; fi
    _prefix=$(dirname "$(dirname "$(command -v protoc)")")
    for _d in "$_prefix/include" /usr/include /usr/local/include; do
        if [ -f "$_d/google/protobuf/empty.proto" ]; then return 0; fi
    done
    return 1
}

missing_build_tools() {
    _m=""
    have cc || have gcc || have clang || _m="$_m cc"
    have_protoc || _m="$_m protoc"
    { have node && have npm; } || _m="$_m node"
    printf '%s' "$_m"
}

check_disk() {
    _dir="${CARGO_HOME:-$HOME/.cargo}"
    while [ ! -d "$_dir" ] && [ "$_dir" != "/" ]; do _dir=$(dirname "$_dir"); done
    _kb=$(df -Pk "$_dir" 2>/dev/null | awk 'NR==2 {print $4}') || return 0
    case "$_kb" in ''|*[!0-9]*) return 0 ;; esac
    _gb=$((_kb / 1024 / 1024))
    if [ "$_gb" -lt "$DISK_WANT_GB" ]; then
        warn "$_gb GB free on $_dir, and a cold build wants about $DISK_WANT_GB GB.
       It compiles arrow, datafusion and lance."
    fi
}

# Everything between "cargo is installed" and "the build gets past the build scripts".
# Each of these otherwise surfaces a few hundred lines into a build script's output, which
# is a bad place to learn that a package was missing.
check_build_tools() {
    if [ "$OS" = macos ] && ! xcode-select -p >/dev/null 2>&1; then
        die "the Xcode command line tools are not installed. \`xcode-select --install\`"
    fi

    _need=$(missing_build_tools)

    if [ -n "$_need" ] && [ "$WITH_DEPS" = 1 ] && [ -n "$PM" ]; then
        PKGS=""
        for _item in $_need; do add_pkgs "$(pkg_for "$_item")"; done
        if [ -n "$PKGS" ]; then
            step "installing the build toolchain:$PKGS"
            # shellcheck disable=SC2086
            $PM_INSTALL $PKGS || warn "that failed — the report below still stands"
            _need=$(missing_build_tools)
        fi
    fi

    if [ -n "$_need" ]; then
        say ""
        printf '%serror%s this host cannot build Centinel yet:\n\n' "$R" "$N" >&2
        PKGS=""
        for _item in $_need; do
            printf '      %-10s %s\n' "$_item" "$(why_for "$_item")" >&2
            add_pkgs "$(pkg_for "$_item")"
        done
        say ""
        if [ -n "$PKGS" ]; then
            say "      $PM_INSTALL$PKGS"
            say ""
            note "or re-run this script with --deps"
        else
            say "      Install them however this host does — no package manager was found."
        fi
        say ""
        exit 1
    fi

    node_version=$(node --version | sed 's/^v//')
    node_major=$(printf '%s' "$node_version" | cut -d. -f1)
    node_supported=0
    if [ "$node_major" -ge 22 ] && { [ "$node_major" -gt 22 ] || version_ge "$node_version" "22.13.0"; }; then
        node_supported=1
    fi
    [ "$node_supported" = 1 ] || die "Node $node_version cannot build the Centinel
      browser workspace. Install Node 22.13+, then re-run this."

    check_disk
}

install_pkg() {
    pkg=$1

    # `--path` against a clone, `--git` against the repository. Both take `--locked`, so
    # either way the build uses the Cargo.lock that was tested rather than resolving fresh.
    set -- install --locked --force
    if [ -n "$SRC" ]; then
        set -- "$@" --path "$SRC/crates/$pkg"
    else
        set -- "$@" --git "$REPO"
        if [ -n "$TAG" ]; then set -- "$@" --tag "$TAG"; fi
        set -- "$@" "$pkg"
    fi
    if [ -n "$CARGO_ROOT" ]; then set -- "$@" --root "$CARGO_ROOT"; fi

    step "building $pkg"
    cargo "$@" || die "$pkg failed to build"
}

build_from_source() {
    # A tag names a revision of the repository, so it cannot mean anything against a
    # working tree. Saying so beats building the checkout and reporting the tag that was
    # ignored.
    if [ -n "$SRC" ] && [ -n "$TAG" ]; then
        die "--tag applies to a git build, and this is a clone at $SRC.
      \`git checkout $TAG\` first, or run the installer without a clone beside it."
    fi

    case "$BIN_DIR" in
        */bin) CARGO_ROOT=$(dirname "$BIN_DIR") ;;
        *) die "--bin-dir must end in \`bin\` for a source build: cargo installs into
      <root>/bin and this script does not move what cargo placed." ;;
    esac
    # Left empty when the default was used, so cargo applies its own root and honours
    # CARGO_INSTALL_ROOT the way it always has.
    if [ "$BIN_DIR" = "${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin" ]; then
        CARGO_ROOT=""
    fi

    step "checking this host"
    check_rust
    check_build_tools
    note "$OS/$ARCH"

    # ------------------------------------------------------------ cpu tuning

    # Tuned for this CPU by default, because this script installs to the machine it just
    # built on — the one situation where native is unambiguously right, and it lets rustc
    # use every instruction this CPU has. --portable is for the other situation, and it is
    # how the release binaries are built.
    case "${RUSTFLAGS:-}" in
        *target-cpu=*)
            # Left alone rather than appended to: somebody who set one meant it.
            TUNING="$(printf '%s' "$RUSTFLAGS" | sed 's/.*target-cpu=\([^ ]*\).*/\1/') — from your RUSTFLAGS" ;;
        *)
            if [ "$NATIVE" = 1 ]; then
                RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=native"
                export RUSTFLAGS
                TUNING="native — this host's CPU only"
            else
                TUNING="portable — any CPU of this architecture"
            fi ;;
    esac

    # ------------------------------------------------------------ build

    if [ -n "$SRC" ]; then
        SOURCE_NOTE="$SRC"
    elif [ -n "$TAG" ]; then
        SOURCE_NOTE="$REPO @ $TAG"
    else
        SOURCE_NOTE="$REPO"
    fi

    say ""
    step "building centinel"
    note "from         $SOURCE_NOTE"
    note "cpu tuning   $TUNING"
    note "into         $BIN_DIR"
    say ""

    install_pkg "$PKG_MAIN"
}

# ---------------------------------------------------------------- install

WAS=$(installed_version)
DONE=""

# A clone installs the clone. That is the promise the clone path is for, and a download
# would quietly install master over the change somebody is testing — so a clone downloads
# only when it is asked to in as many words.
if [ "$METHOD" = download ] || { [ "$METHOD" = auto ] && [ -z "$SRC" ]; }; then
    ASSET=$(asset_for_host)
    if [ -n "$ASSET" ]; then
        if try_download "$ASSET"; then
            DONE=download
        elif [ "$METHOD" = download ]; then
            die "no release binary was installed, and --download says not to build."
        else
            say ""
            note "building from source instead"
        fi
    elif [ "$METHOD" = download ]; then
        die "this host has no release binary. Drop --download to build from source."
    fi
fi

if [ -z "$DONE" ]; then
    build_from_source
    DONE=build
fi

# ---------------------------------------------------------------- verify

say ""
step "checking the install"

[ -x "$BIN_DIR/$PKG_MAIN" ] || die "$PKG_MAIN is not in $BIN_DIR after an install that
      reported success."
note "$PKG_MAIN in $BIN_DIR"

# What an install before the inference servers left behind. Named rather than deleted: the
# weights are gigabytes somebody may want to hand to llama-server, and this script removes
# nothing it did not put there this run.
if [ -e "$BIN_DIR/$OLD_WORKER" ]; then
    note "$BIN_DIR/$OLD_WORKER is no longer used — transcription goes to a server now"
    note "  rm $BIN_DIR/$OLD_WORKER"
fi
if [ "$OS" = macos ]; then
    OLD_MODELS="${CENTINEL_MODELS:-$HOME/Library/Caches/centinel/models}"
else
    OLD_MODELS="${CENTINEL_MODELS:-${XDG_CACHE_HOME:-$HOME/.cache}/centinel/models}"
fi
if [ -d "$OLD_MODELS" ]; then
    _size=$(du -sh "$OLD_MODELS" 2>/dev/null | awk '{print $1}')
    note "the model weights in $OLD_MODELS (${_size:-?}) are no longer read by centinel"
    note "  serve them with llama-server or whisper.cpp's server, or: rm -r $OLD_MODELS"
fi

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        warn "$BIN_DIR is not on your PATH. Add it:"
        say ""
        say "      export PATH=\"$BIN_DIR:\$PATH\""
        say "" ;;
esac

# An older copy earlier on PATH answers `centinel` instead of the one just installed, and
# the symptom is an update that appears to have done nothing.
FOUND=$(command -v "$PKG_MAIN" 2>/dev/null || true)
if [ -n "$FOUND" ] && [ "$FOUND" != "$BIN_DIR/$PKG_MAIN" ]; then
    warn "another centinel is earlier on your PATH, and it is the one that will run:"
    say ""
    say "      $FOUND"
    say ""
fi

# ---------------------------------------------------------------- external tools

missing_tools=""
have ffmpeg || missing_tools="$missing_tools ffmpeg"
have yt-dlp || missing_tools="$missing_tools yt-dlp"

if [ -n "$missing_tools" ]; then
    if [ -z "$PM_INSTALL" ]; then
        warn "missing:$missing_tools. Both are required — ffmpeg decodes audio for
       transcription and yt-dlp acquires YouTube. No package manager was found here, so
       install them however this host does."
    elif [ "$WITH_DEPS" = 1 ]; then
        step "installing$missing_tools"
        # shellcheck disable=SC2086
        $PM_INSTALL $missing_tools || warn "installing$missing_tools failed — do it by hand"
    else
        warn "missing:$missing_tools. Both are required. Install them with:"
        say ""
        say "      $PM_INSTALL$missing_tools"
        say ""
        note "or re-run this script with --deps"
    fi
fi

# ---------------------------------------------------------------- what is next

NOW=$(installed_version)

say ""
step "installed"
if [ -n "$WAS" ] && [ -n "$NOW" ] && [ "$WAS" != "$NOW" ]; then
    note "$PKG_MAIN $WAS -> $NOW"
else
    note "$PKG_MAIN ${NOW:-(version unknown)}"
fi
if [ "$DONE" = download ]; then
    note "a release binary, built for the baseline CPU of this architecture"
fi
say ""

if [ "$RUN_DOCTOR" = 1 ]; then
    # Run by path: PATH may not carry the bin directory yet, and doctor's verdict is the
    # one that counts — this script checks that the binaries landed, doctor checks whether
    # the machine can actually run a collection.
    "$BIN_DIR/$PKG_MAIN" doctor || true
    say ""
fi

say "  Web and remote MCP serving requires Node.js 22.13+ on this host."
say "  ${G}Next${N}"
say "    start the servers [inference] names in centinel.toml"
say "      ${D}llama-server for embed and rerank, whisper.cpp's server for transcription${N}"
say "    centinel doctor         ${D}what is still missing, and the fix for each gap${N}"
say ""
