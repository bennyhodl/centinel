#!/bin/sh
#
# Centinel installer. Run it from a clone:
#
#   git clone https://github.com/bennyhodl/centinel
#   cd centinel
#   ./install.sh
#
# Centinel is TWO executables and needs both. `centinel` links llama.cpp and
# `centinel-whisper` links whisper.cpp; linked into one binary they resolve to one copy of
# ggml and transcription silently returns nothing (README, "Why two"). `centinel` finds the
# worker beside itself first, so the one thing this script must not get wrong is putting
# both in the same directory — which is why it installs them itself rather than printing two
# commands and trusting the reader to run both.
#
# There is no prebuilt binary to download. Both packages compile a C++ library from source,
# so the binary correct for a host is the one built on it, and the only thing that varies
# per host is the GPU backend — which is what --accel selects.
#
# Nothing is prompted, so an unattended run behaves the same as an attended one. Every
# choice is a flag or an environment variable.

set -eu

# The workspace `rust-version`. LanceDB sets it; see the root Cargo.toml.
MSRV="1.91"

PKG_MAIN="centinel"
PKG_WORKER="centinel-whisper"

# The clone this script was run from, not the working directory — `../centinel/install.sh`
# has to install the same thing `./install.sh` does.
SRC=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

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
${B}Centinel installer${N} — builds and installs both binaries into one directory.

  ./install.sh
  ./install.sh --accel cuda --deps

${B}Options${N}
  --accel <auto|none|cuda|vulkan|rocm>
                     GPU backend. Default auto: Metal on macOS (built in, no flag),
                     CUDA or ROCm on Linux when the toolchain is on PATH, else none.
  --bin-dir <dir>    Where the two binaries go. Must end in \`bin\` — cargo owns the
                     layout under its install root. Default: \$CARGO_HOME/bin.
  --portable         Build for the baseline CPU of this architecture instead of this
                     host's. Slower — it is what makes the binary copyable to another
                     machine. Default is to tune for the CPU that is building.
  --deps             Also install ffmpeg and yt-dlp, using the package manager found on
                     this host. Without it they are only reported.
  --no-doctor        Skip the closing \`centinel doctor\` run.
  -h, --help         This text.

${B}Environment${N}
  CENTINEL_ACCEL, CENTINEL_BIN_DIR — same as the flags above.
  CENTINEL_NATIVE=0 — same as --portable.

${B}What it does not do${N}
  Install Rust. If \`cargo\` is missing it prints the one command that fixes that and stops.
EOF
}

# ---------------------------------------------------------------- arguments

ACCEL="${CENTINEL_ACCEL:-auto}"
BIN_DIR="${CENTINEL_BIN_DIR:-}"
NATIVE="${CENTINEL_NATIVE:-1}"
WITH_DEPS=0
RUN_DOCTOR=1

while [ $# -gt 0 ]; do
    case "$1" in
        --accel)   [ $# -ge 2 ] || die "--accel needs a value"; ACCEL=$2; shift 2 ;;
        --bin-dir) [ $# -ge 2 ] || die "--bin-dir needs a value"; BIN_DIR=$2; shift 2 ;;
        --portable)  NATIVE=0; shift ;;
        --deps)      WITH_DEPS=1; shift ;;
        --no-doctor) RUN_DOCTOR=0; shift ;;
        -h|--help)   usage; exit 0 ;;
        *) usage; die "unknown option: $1" ;;
    esac
done

[ -f "$SRC/crates/$PKG_MAIN/Cargo.toml" ] ||
    die "$SRC is not a Centinel clone. Run this script from inside one:

      git clone https://github.com/bennyhodl/centinel
      cd centinel
      ./install.sh"

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

# ---------------------------------------------------------------- preflight

missing_cxx=""

check_rust() {
    have cargo && have rustc || die "no Rust toolchain. Install one, then re-run this:

      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"

    rustc_version=$(rustc --version | awk '{print $2}' | cut -d- -f1)
    version_ge "$rustc_version" "$MSRV" || die "Rust $rustc_version is too old — Centinel
      needs $MSRV or newer. \`rustup update stable\` fixes it."
    note "rust $rustc_version"
}

check_cxx() {
    # Both packages build a vendored C++ library through cmake. A missing compiler surfaces
    # a few hundred lines into a build script's output, which is a bad place to learn it.
    have cmake || missing_cxx="$missing_cxx cmake"
    have cc || have gcc || have clang || missing_cxx="$missing_cxx a C compiler"
    have c++ || have g++ || have clang++ || missing_cxx="$missing_cxx a C++ compiler"

    if [ "$OS" = macos ] && ! xcode-select -p >/dev/null 2>&1; then
        die "the Xcode command line tools are not installed. \`xcode-select --install\`"
    fi

    [ -z "$missing_cxx" ] || die "missing a C++ toolchain:$missing_cxx.
      \`centinel\` compiles llama.cpp and \`$PKG_WORKER\` compiles whisper.cpp, so this is
      not optional. On Debian/Ubuntu: sudo apt install build-essential cmake"
}

step "checking this host"
check_rust
check_cxx
note "$OS/$ARCH"

# ---------------------------------------------------------------- accelerator

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
        [ "$OS" = macos ] || die "metal is macOS only"
        ACCEL_NOTE="metal (built in on macOS, no feature flag)" ;;
    cuda|rocm|vulkan)
        if [ "$OS" = macos ]; then die "$ACCEL is not available on macOS — use --accel metal"; fi
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
    *)
        die "unknown --accel value: $ACCEL" ;;
esac

# ---------------------------------------------------------------- cpu tuning

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
# situation where native is unambiguously right. --portable is for the other one.
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

# ---------------------------------------------------------------- destination

if [ -n "$BIN_DIR" ]; then
    case "$BIN_DIR" in
        */bin|bin) ;;
        *) die "--bin-dir must end in \`bin\`: cargo installs into <root>/bin and this
      script does not move what cargo placed." ;;
    esac
    CARGO_ROOT=$(dirname "$BIN_DIR")
else
    CARGO_ROOT=""
    BIN_DIR="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin"
fi

# ---------------------------------------------------------------- install

say ""
step "installing centinel"
note "from         $SRC"
note "accelerator  $ACCEL_NOTE"
note "cpu tuning   $TUNING"
note "into         $BIN_DIR"
say ""

install_pkg() {
    pkg=$1

    set -- install --locked --force --path "$SRC/crates/$pkg"
    if [ -n "$CARGO_ROOT" ]; then set -- "$@" --root "$CARGO_ROOT"; fi
    if [ -n "$FEATURE" ]; then set -- "$@" --features "$FEATURE"; fi

    step "building $pkg"
    cargo "$@" || die "$pkg failed to build"
}

# The worker first. It is the cheaper of the two C++ builds, so a toolchain that cannot
# compile whisper.cpp says so in a few minutes rather than after llama.cpp and LanceDB.
install_pkg "$PKG_WORKER"
install_pkg "$PKG_MAIN"

# ---------------------------------------------------------------- verify

say ""
step "checking the install"

for bin in "$PKG_MAIN" "$PKG_WORKER"; do
    [ -x "$BIN_DIR/$bin" ] || die "$bin is not in $BIN_DIR after a build that reported
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

# ---------------------------------------------------------------- external tools

# Only ever names a manager that is actually installed here. `doctor` leaves the fix for
# ffmpeg and yt-dlp empty on purpose — a guessed `brew install` is wrong on most machines —
# and a detected manager is not a guess.
pkg_manager() {
    if   have brew;    then echo "brew install"
    elif have apt-get; then echo "sudo apt-get install -y"
    elif have dnf;     then echo "sudo dnf install -y"
    elif have pacman;  then echo "sudo pacman -S --noconfirm"
    elif have zypper;  then echo "sudo zypper install -y"
    elif have apk;     then echo "sudo apk add"
    fi
}

missing_tools=""
have ffmpeg || missing_tools="$missing_tools ffmpeg"
have yt-dlp || missing_tools="$missing_tools yt-dlp"

if [ -n "$missing_tools" ]; then
    manager=$(pkg_manager)
    if [ -z "$manager" ]; then
        warn "missing:$missing_tools. Both are required — ffmpeg decodes audio for
       transcription and yt-dlp acquires YouTube. No package manager was found here, so
       install them however this host does."
    elif [ "$WITH_DEPS" = 1 ]; then
        step "installing$missing_tools"
        # shellcheck disable=SC2086
        $manager $missing_tools || warn "installing$missing_tools failed — do it by hand"
    else
        warn "missing:$missing_tools. Both are required. Install them with:"
        say ""
        say "      $manager$missing_tools"
        say ""
        note "or re-run this script with --deps"
    fi
fi

# ---------------------------------------------------------------- what is next

say ""
step "installed"
note "$("$BIN_DIR/$PKG_MAIN" --version 2>/dev/null || echo "$PKG_MAIN")"
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
