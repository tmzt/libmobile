#!/usr/bin/env bash
# Build (and optionally install + launch) a libmobile example APK.
#
#   examples/build.sh bitmap|wgpu|audiocapture|securestore [--install] [-- <extra cargo args>]
#
# Two stages, and no Gradle files in the repo: the example's build.rs writes
# the whole Gradle project (libmobile::gradle) while cargo builds the .so, and
# gradle then packages it.
#
# Needs: rustup target aarch64-linux-android, an Android SDK (platform 35) with
# an NDK, gradle 9.x and a JDK 17/21. Override discovery with ANDROID_HOME,
# ANDROID_NDK_ROOT and JAVA_HOME.
set -euo pipefail

EXAMPLE="${1:?usage: examples/build.sh bitmap|wgpu|audiocapture|securestore [--install] [-- cargo args]}"
shift
INSTALL=0
CARGO_EXTRA=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --install) INSTALL=1; shift ;;
        --) shift; CARGO_EXTRA=("$@"); break ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE="libmobile-example-$EXAMPLE"
TARGET=aarch64-linux-android
API=29

# ── toolchain discovery ──────────────────────────────────────────────────────
if [[ -z "${ANDROID_HOME:-}" ]]; then
    for candidate in "$HOME/Library/Android/sdk" /opt/homebrew/share/android-commandlinetools "$HOME/Android/Sdk"; do
        [[ -d "$candidate" ]] && ANDROID_HOME="$candidate" && break
    done
fi
[[ -d "${ANDROID_HOME:-}" ]] || { echo "error: set ANDROID_HOME" >&2; exit 1; }
export ANDROID_HOME
if [[ -z "${ANDROID_NDK_ROOT:-}" ]]; then
    ANDROID_NDK_ROOT="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
fi
[[ -d "${ANDROID_NDK_ROOT:-}" ]] || { echo "error: no NDK under $ANDROID_HOME/ndk" >&2; exit 1; }
if [[ -z "${JAVA_HOME:-}" ]]; then
    for candidate in /opt/homebrew/opt/openjdk@21 /opt/homebrew/opt/openjdk@17; do
        [[ -d "$candidate" ]] && JAVA_HOME="$candidate" && break
    done
fi
[[ -n "${JAVA_HOME:-}" ]] && export JAVA_HOME

HOST_TAG="$(uname -s | tr '[:upper:]' '[:lower:]')-x86_64" # the NDK ships x86_64 host tools only
LLVM="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/$HOST_TAG/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LLVM/${TARGET}${API}-clang"
export CC_aarch64_linux_android="$LLVM/${TARGET}${API}-clang"
export AR_aarch64_linux_android="$LLVM/llvm-ar"

# ── 1. Rust (also generates the Gradle project) ──────────────────────────────
echo "==> cargo build -p $PACKAGE ($TARGET, release)"
cargo build --manifest-path "$ROOT/Cargo.toml" -p "$PACKAGE" --lib --release --target "$TARGET" ${CARGO_EXTRA[@]+"${CARGO_EXTRA[@]}"}

# ── 2. Gradle (packaging only) ───────────────────────────────────────────────
PROJECT="${LIBMOBILE_GRADLE_PROJECT_DIR:-$ROOT/target/$TARGET/release/gradle/$PACKAGE}"
echo "==> gradle assembleDebug ($PROJECT)"
gradle -p "$PROJECT" --quiet assembleDebug
APK="$PROJECT/app/build/outputs/apk/debug/app-debug.apk"
echo "==> $APK"

if [[ "$INSTALL" == 1 ]]; then
    APP_ID="dev.highbay.libmobile.example.$EXAMPLE"
    adb install -r "$APK"
    adb shell am start -n "$APP_ID/dev.highbay.libmobile.MobileNativeActivity"
    echo "==> logs: adb logcat -s libmobile-$EXAMPLE libmobile"
fi
