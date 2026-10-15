#!/usr/bin/env bash
# Builds libactualwave_swc.so for the given ABIs (default arm64-v8a) into android/src/main/jniLibs.
# Needs: rustup, cargo-ndk (cargo install cargo-ndk), Android NDK (ANDROID_NDK_HOME).
set -euo pipefail
cd "$(dirname "$0")/../rust"
ABIS=("${@:-arm64-v8a}")
TARGETS=()
for abi in "${ABIS[@]}"; do
  case "$abi" in
    arm64-v8a) rustup target add aarch64-linux-android ;;
    armeabi-v7a) rustup target add armv7-linux-androideabi ;;
    x86_64) rustup target add x86_64-linux-android ;;
  esac
  TARGETS+=(-t "$abi")
done
cargo ndk "${TARGETS[@]}" -o ../android/src/main/jniLibs build --release
