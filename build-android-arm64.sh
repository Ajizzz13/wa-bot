#!/usr/bin/env bash
set -euo pipefail

: "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to the Android NDK directory}"

bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
clang="$bin/aarch64-linux-android24-clang"

export CC_aarch64_linux_android="$clang"
export CXX_aarch64_linux_android="$bin/aarch64-linux-android24-clang++"
export AR_aarch64_linux_android="$bin/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$clang"

cargo +nightly build --release --target aarch64-linux-android
