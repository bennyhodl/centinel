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
# There are two ways to end up with the binaries, and this script picks one:
#
#   download   a release asset, when a release carries one this host can run
#   build      from source, which is every other host and the fallback for every failure
#
# A release carries two assets, and both are GPU builds. Embedding is the stage measured in
# days and it is what the whole tool is for, so a CPU-only download would hand somebody the
# slow half of Centinel and call it an install:
#
#   aarch64-apple-darwin              Metal, which is built into any macOS build
#   x86_64-unknown-linux-gnu, CUDA    wants an NVIDIA driver and the CUDA runtime
#
# Nothing about the download is load-bearing. No asset for this host, no release yet, a
# checksum that does not match, a binary that will not start — each falls back to the build
# this script did before it could download anything, so the worst a broken release does is
# cost one HTTP request. That is also why the download is tried before Rust is checked for:
# a host that downloads does not need a toolchain at all.
#
# Centinel is TWO executables and needs both. `centinel` links llama.cpp and
# `centinel-whisper` links whisper.cpp; linked into one binary they resolve to one copy of
# ggml and transcription silently returns nothing (README, "Why two"). `centinel` finds the
# worker beside itself first, so the one thing this script must not get wrong is putting
# both in the same directory — which is why it installs them itself rather than printing two
# commands and trusting the reader to run both.
#
# Nothing is prompted, so an unattended run behaves the same as an attended one. Every
# choice is a flag or an environment variable.

set -eu

# The workspace `rust-version`. LanceDB sets it; see the root Cargo.toml.
MSRV="1.91"

PKG_MAIN="centinel"
PKG_WORKER="centinel-whisper"

REPO="https://github.com/bennyhodl/centinel"

# Release asset names. No version in the name on purpose: it makes
# `releases/latest/download/<name>` a fixed URL, so updating is the same command as
# installing rather than a second thing to remember.
ASSET_MACOS_ARM64="centinel-aarch64-apple-darwin.tar.gz"
ASSET_LINUX_CUDA="centinel-x86_64-unknown-linux-gnu-cuda.tar.gz"

# A cold build unpacks and compiles llama.cpp, whisper.cpp, arrow, datafusion and lance.
# Below this it is close enough that "no space left on device" is a real way to lose half
# an hour, and cargo does not check first.
DISK_WANT_GB=15

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
${B}Centinel installer${N} — puts both binaries in one directory, downloading a release
binary when this host can run one and building from source when it cannot.

  curl --proto '=https' --tlsv1.2 -sSf $REPO/raw/master/install.sh | sh
  curl ... | sh -s -- --accel cuda --deps      ${D}flags, through the pipe${N}
  ./install.sh --accel cuda --deps             ${D}from a clone${N}

Re-run the same command to update. It reads the version off the latest release and stops
early when that is the one already installed.

${B}Options${N}
  --build            Build from source, even where a release binary would fit. This is
                     already what a clone does — it installs the clone.
  --download         Download a release binary, and fail rather than quietly building
                     when this host has no asset. Works from a clone too.
  --force            Install again even when the release is the version already here.
  --tag <tag>        A released tag rather than the latest release or the default branch.
  --accel <auto|none|cuda|vulkan|rocm>
                     GPU backend. Default auto: Metal on macOS (built in, no flag),
                     CUDA or ROCm on Linux when the toolchain is on PATH, else none.
  --bin-dir <dir>    Where the two binaries go. A source build must end in \`bin\` —
                     cargo owns the layout under its install root.
  --portable         Build for the baseline CPU of this architecture instead of this
                     host's. Slower — it is what makes the binary copyable to another
                     machine. Default is to tune for the CPU that is building. Downloaded
                     binaries are built this way already.
  --deps             Also install what is missing, using the package manager found on
                     this host: ffmpeg and yt-dlp, and the C++ toolchain when building.
                     Without it they are only reported.
  --no-doctor        Skip the closing \`centinel doctor\` run.
  -h, --help         This text.

${B}Environment${N}
  CENTINEL_ACCEL, CENTINEL_BIN_DIR, CENTINEL_TAG — same as the flags above.
  CENTINEL_METHOD=auto|download|build — same as --download and --build.
  CENTINEL_NATIVE=0 — same as --portable.

${B}What it does not do${N}
  Install Rust. A download does not need it. If a build does and \`cargo\` is missing, it
  prints the one command that fixes that and stops.
EOF
}

# ---------------------------------------------------------------- arguments

ACCEL="${CENTINEL_ACCEL:-auto}"
BIN_DIR="${CENTINEL_BIN_DIR:-}"
METHOD="${CENTINEL_METHOD:-auto}"
NATIVE="${CENTINEL_NATIVE:-1}"
TAG="${CENTINEL_TAG:-}"
FORCE=0
WITH_DEPS=0
RUN_DOCTOR=1

while [ $# -gt 0 ]; do
    case "$1" in
        --accel)   [ $# -ge 2 ] || die "--accel needs a value"; ACCEL=$2; shift 2 ;;
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
        die "this script needs a POSIX shell. On Windows, run the two \`cargo install\`
      commands from the README by hand — into the same directory." ;;
    *) die "unsupported system: $(uname -s)" ;;
esac

ARCH=$(uname -m)
case "$ARCH" in
    arm64|aarch64) ARCH=arm64 ;;
    x86_64|amd64)  ARCH=x86_64 ;;
    *) warn "untested architecture $ARCH — the build decides whether it works" ;;
esac

# Rejected here rather than in the accelerator block below, because an impossible pair is
# an error about the command that was typed, and it should not wait behind a download.
case "$ACCEL" in
    auto|none|cuda|vulkan|rocm) ;;
    metal) [ "$OS" = macos ] || die "metal is macOS only" ;;
    *) die "unknown --accel value: $ACCEL" ;;
esac
case "$OS/$ACCEL" in
    macos/cuda|macos/vulkan|macos/rocm)
        die "$ACCEL is not available on macOS — use --accel metal, or leave it to auto" ;;
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
# on macOS the compilers and libclang come from the Xcode command line tools, which is a
# different command and is checked separately.
pkg_for() {
    case "$PM:$1" in
        brew:cmake)              echo cmake ;;
        brew:protoc)             echo protobuf ;;
        apt:cmake)               echo cmake ;;
        apt:cc|apt:c++)          echo build-essential ;;
        apt:libclang)            echo libclang-dev ;;
        apt:protoc)              echo "protobuf-compiler libprotobuf-dev" ;;
        dnf:cmake)               echo cmake ;;
        dnf:cc|dnf:c++)          echo "gcc gcc-c++ make" ;;
        dnf:libclang)            echo clang-devel ;;
        dnf:protoc)              echo "protobuf-compiler protobuf-devel" ;;
        pacman:cmake)            echo cmake ;;
        pacman:cc|pacman:c++)    echo base-devel ;;
        pacman:libclang)         echo clang ;;
        pacman:protoc)           echo protobuf ;;
        zypper:cmake)            echo cmake ;;
        zypper:cc|zypper:c++)    echo "gcc gcc-c++ make" ;;
        zypper:libclang)         echo clang-devel ;;
        zypper:protoc)           echo "protobuf-devel" ;;
        apk:cmake)               echo cmake ;;
        apk:cc|apk:c++)          echo build-base ;;
        apk:libclang)            echo clang-dev ;;
        apk:protoc)              echo protobuf-dev ;;
    esac
}

# Why each one is needed, in the report. A person who is about to install packages on
# somebody's word deserves to know which dependency asked for them.
why_for() {
    case "$1" in
        cmake)    echo "builds the vendored llama.cpp and whisper.cpp" ;;
        cc)       echo "a C compiler" ;;
        c++)      echo "a C++ compiler" ;;
        libclang) echo "bindgen reads llama.cpp's and whisper.cpp's headers with it" ;;
        protoc)   echo "six lance crates run prost-build, and none of them vendor it" ;;
    esac
}

PKGS=""
add_pkgs() {
    for _p in $1; do
        case " $PKGS " in *" $_p "*) ;; *) PKGS="$PKGS $_p" ;; esac
    done
}

# ---------------------------------------------------------------- destination

# Where a previous install put them, when there was one. A download that lands somewhere
# else does not replace the old binaries, it shadows them — and then PATH order decides
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

# This asks a different question from the accelerator block further down, and the
# difference is the point. A release binary needs the NVIDIA *driver*; building CUDA needs
# the *toolkit*. A host with a card and no nvcc is exactly the host a download serves
# best, and `auto` would otherwise send it to a CPU build.

cpu_has() { grep -qw "$1" /proc/cpuinfo 2>/dev/null; }

nvidia_driver() {
    if [ -e /proc/driver/nvidia/version ]; then return 0; fi
    if have nvidia-smi; then return 0; fi
    return 1
}

# `centinel` links cuBLAS statically, so it needs nothing but the driver. `centinel-whisper`
# does not — whisper-rs-sys links `-lcudart -lcublas -lcublasLt` — so the host needs the
# CUDA runtime. Shipping those in the asset would be most of a gigabyte, which is why this
# is a check and not a bundle.
cuda_runtime() {
    for _d in /usr/local/cuda/lib64 /usr/local/cuda-12*/lib64 \
              /usr/lib/x86_64-linux-gnu /usr/lib64 /opt/cuda/lib64; do
        for _f in "$_d"/libcublas.so.12*; do
            if [ -e "$_f" ]; then return 0; fi
        done
    done
    for _ldconfig in ldconfig /sbin/ldconfig; do
        if command -v "$_ldconfig" >/dev/null 2>&1; then
            if "$_ldconfig" -p 2>/dev/null | grep -q 'libcublas\.so\.12'; then return 0; fi
        fi
    done
    return 1
}

# The asset for this host, or empty with a reason on stderr. Empty is not a failure: it
# is the answer "build", which is what this script did for every host before releases
# carried binaries at all.
asset_for_host() {
    case "$OS/$ARCH" in
        macos/arm64)
            case "$ACCEL" in
                auto|metal) printf '%s' "$ASSET_MACOS_ARM64" ;;
                *) note "no release binary for --accel $ACCEL on macOS" ;;
            esac ;;
        linux/x86_64)
            case "$ACCEL" in
                auto|cuda) ;;
                *) note "the Linux release binary is a CUDA build; --accel $ACCEL builds"
                   return 0 ;;
            esac
            # Built for x86-64-v3, which is AVX2 and FMA — any x86_64 since about 2013.
            # Older than that and the binary faults on an instruction rather than saying
            # anything useful, so it is not offered.
            if ! cpu_has avx2; then
                note "this CPU has no AVX2, and the release binary is built for it"
                return 0
            fi
            if ! nvidia_driver; then
                note "no NVIDIA driver here, and the Linux release binary is a CUDA build"
                return 0
            fi
            if ! cuda_runtime; then
                note "the CUDA runtime is not installed, and centinel-whisper links it"
                note "  ${PM_INSTALL:-your package manager} cuda-runtime-12-4  ${D}(~150 MB, no compiler)${N}"
                return 0
            fi
            printf '%s' "$ASSET_LINUX_CUDA" ;;
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

    for _b in "$PKG_MAIN" "$PKG_WORKER"; do
        if [ ! -f "$TMP/$_b" ]; then
            warn "$_asset does not contain $_b, and both are needed"
            return 1
        fi
        chmod +x "$TMP/$_b"
    done

    # curl sets no quarantine attribute, but a browser download does, and clearing one
    # that is not there costs nothing.
    if [ "$OS" = macos ] && have xattr; then
        xattr -d com.apple.quarantine "$TMP/$PKG_MAIN" "$TMP/$PKG_WORKER" 2>/dev/null || true
    fi

    # The check that catches what the checks above cannot: a glibc too old, a CUDA runtime
    # that is present but wrong, a macOS older than the one that built it. The process
    # starts, the dynamic linker resolves everything, and it prints a version — and if it
    # does not, a build is still on the table and nothing has been written yet.
    if ! "$TMP/$PKG_MAIN" --version >/dev/null 2>&1 \
    || ! "$TMP/$PKG_WORKER" --version >/dev/null 2>&1; then
        warn "the release binary does not run on this host"
        return 1
    fi

    mkdir -p "$BIN_DIR" || return 1

    # Into place by rename, which is atomic within a directory and, unlike writing over
    # the file, does not fail on a binary that is running right now.
    for _b in "$PKG_MAIN" "$PKG_WORKER"; do
        cp "$TMP/$_b" "$BIN_DIR/.$_b.new" || return 1
        chmod 755 "$BIN_DIR/.$_b.new"
        mv "$BIN_DIR/.$_b.new" "$BIN_DIR/$_b" || return 1
    done

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

# bindgen wants the shared library, not the `clang` binary — a host can have gcc and clang
# and still be missing libclang.so, which is `libclang-dev` on Debian and its relatives.
have_libclang() {
    if [ -n "${LIBCLANG_PATH:-}" ]; then return 0; fi
    # Answered by the Xcode command line tools, which are checked as one thing below.
    if [ "$OS" = macos ]; then return 0; fi
    for _d in /usr/lib /usr/lib64 /usr/local/lib /usr/lib/x86_64-linux-gnu \
              /usr/lib/aarch64-linux-gnu /usr/lib/llvm-*/lib; do
        for _f in "$_d"/libclang.so*; do
            if [ -e "$_f" ]; then return 0; fi
        done
    done
    for _ldconfig in ldconfig /sbin/ldconfig; do
        if command -v "$_ldconfig" >/dev/null 2>&1; then
            if "$_ldconfig" -p 2>/dev/null | grep -q libclang; then return 0; fi
        fi
    done
    return 1
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
    have cmake || _m="$_m cmake"
    have cc || have gcc || have clang || _m="$_m cc"
    have c++ || have g++ || have clang++ || _m="$_m c++"
    have_libclang || _m="$_m libclang"
    have_protoc || _m="$_m protoc"
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
       It compiles llama.cpp, whisper.cpp, arrow, datafusion and lance."
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
    if [ -n "$FEATURE" ]; then set -- "$@" --features "$FEATURE"; fi

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

    # ------------------------------------------------------------ accelerator

    # The feature name, empty when nothing is passed to cargo. Metal is empty on purpose:
    # centinel-core enables it per-target on macOS, so asking for it by flag is an error.
    FEATURE=""
    ACCEL_NOTE=""

    case "$ACCEL" in
        auto)
            if [ "$OS" = macos ]; then
                ACCEL=metal
            elif have nvcc; then
                ACCEL=cuda
            elif have hipcc; then
                ACCEL=rocm
            else
                ACCEL=none
            fi ;;
    esac

    case "$ACCEL" in
        metal)
            ACCEL_NOTE="metal (built in on macOS, no feature flag)" ;;
        cuda|rocm|vulkan)
            FEATURE=$ACCEL
            ACCEL_NOTE=$ACCEL
            # Named rather than fatal: a CUDA install with nvcc outside PATH is common, and
            # refusing here would be wrong about a machine that builds fine.
            case "$ACCEL" in
                cuda)   have nvcc  || warn "nvcc is not on PATH; the CUDA build will need it" ;;
                rocm)   have hipcc || warn "hipcc is not on PATH; the ROCm build will need it" ;;
                vulkan) have glslc || warn "glslc is not on PATH; the Vulkan build will need it" ;;
            esac ;;
        none)
            ACCEL_NOTE="none (CPU)" ;;
    esac

    # ------------------------------------------------------------ cpu tuning

    # `target-cpu=native` is not only a Rust codegen flag here. llama-cpp-sys-2 reads
    # target-cpu back out of CARGO_ENCODED_RUSTFLAGS, and without it sets GGML_NATIVE=OFF and
    # derives ggml's instruction-set flags from the *baseline* target features instead — which
    # on x86_64-unknown-linux-gnu are `fxsr,sse,sse2,x87`. That compiles llama.cpp's CPU kernels
    # with no AVX, no AVX2 and no FMA, and nothing recovers it at runtime, because the runtime
    # dispatch (GGML_CPU_ALL_VARIANTS) only comes with a feature this build does not enable. On
    # Linux aarch64 the same build script forces GGML_CPU_ARM_ARCH=armv8-a, dropping dotprod.
    #
    # whisper.cpp is already built this way — whisper-rs-sys passes no target-cpu handling, so
    # ggml's own GGML_NATIVE default (ON for a non-cross build) applies. So the untuned case is
    # not "both a little slow"; it is the transcriber tuned and the embedder not, and embedding
    # is the stage measured in days.
    #
    # Default on because this script installs to the machine it just built on, which is the one
    # situation where native is unambiguously right. --portable is for the other one, and it is
    # how the release binaries are built.
    case "${RUSTFLAGS:-}" in
        *target-cpu=*)
            # Left alone rather than appended to. rustc takes the last target-cpu it is given
            # and llama-cpp-sys takes the first, so adding a second one is how the Rust code and
            # ggml end up tuned for different CPUs with nothing saying so.
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
    note "accelerator  $ACCEL_NOTE"
    note "cpu tuning   $TUNING"
    note "into         $BIN_DIR"
    say ""

    # The worker first. It is the cheaper of the two C++ builds, so a toolchain that cannot
    # compile whisper.cpp says so in a few minutes rather than after llama.cpp and LanceDB.
    install_pkg "$PKG_WORKER"
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

for bin in "$PKG_MAIN" "$PKG_WORKER"; do
    [ -x "$BIN_DIR/$bin" ] || die "$bin is not in $BIN_DIR after an install that reported
      success. Transcription needs both binaries in one directory."
done
note "both binaries in $BIN_DIR"

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

say "  ${G}Next${N}"
say "    centinel models pull    ${D}weights for search and transcription${N}"
say "    centinel doctor         ${D}what is still missing, and the fix for each gap${N}"
say ""
