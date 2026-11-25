#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.2.12479018}"

BUILD_MODE="${1:---release}"
CARGO_FLAG=""
GRADLE_TASK="assembleDebug"
if [[ "$BUILD_MODE" == "--debug" ]]; then
  CARGO_FLAG=""
else
  CARGO_FLAG="--release"
fi

echo "==> Building libvkarma_mobile.so (arm64-v8a $BUILD_MODE)..."
cd "$ROOT_DIR"
cargo ndk -t arm64-v8a -P 31 -o "$ROOT_DIR/android/app/src/main/jniLibs" build $CARGO_FLAG

echo "==> Assembling APK ($GRADLE_TASK)..."
cd "$ROOT_DIR/android"
./gradlew "$GRADLE_TASK"

APK_PATH="$ROOT_DIR/android/app/build/outputs/apk/debug/app-debug.apk"
echo "==> Installing APK on connected device..."
adb install -r "$APK_PATH"

echo "==> Granting MANAGE_EXTERNAL_STORAGE..."
adb shell appops set dev.vkarma.mobile MANAGE_EXTERNAL_STORAGE allow

echo "==> Launching VKarma Mobile..."
adb shell am start -S -n dev.vkarma.mobile/dev.gpui.mobile.GpuiActivity
