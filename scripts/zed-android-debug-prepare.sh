#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ADB_BIN="${ADB:-adb}"
PORT="${LLDB_PORT:-5039}"
PACKAGE_NAME="app.polarbear"
LAUNCH_ACTIVITY="android.app.NativeActivity"
APK_PATH="$ROOT_DIR/target/x/debug/android/gradle/app/build/outputs/apk/debug/app-debug.apk"

if ! command -v "$ADB_BIN" >/dev/null 2>&1; then
  echo "adb not found. Install Android platform-tools or set ADB=... in the environment." >&2
  exit 1
fi

serial="${ANDROID_SERIAL:-$("$ADB_BIN" devices | awk 'NR>1 && $2=="device" {print $1; exit}')}"
if [ -z "$serial" ]; then
  echo "No connected Android devices. Connect a device or start an emulator." >&2
  exit 1
fi

adb_cmd=("$ADB_BIN" -s "$serial")

if [ ! -f "$APK_PATH" ]; then
  echo "APK not found at $APK_PATH" >&2
  echo "Run the Android debug build first (or check your build output path)." >&2
  exit 1
fi

echo "Installing APK..."
"${adb_cmd[@]}" install -r -t "$APK_PATH" >/dev/null

abi_list="$("${adb_cmd[@]}" shell getprop ro.product.cpu.abilist | tr -d '\r')"
if [ -z "$abi_list" ]; then
  abi_list="$("${adb_cmd[@]}" shell getprop ro.product.cpu.abi | tr -d '\r')"
fi
abi="${abi_list%%,*}"

case "$abi" in
  armeabi* ) lldb_arch="arm" ;;
  arm64-v8a ) lldb_arch="aarch64" ;;
  x86 ) lldb_arch="i386" ;;
  x86_64 ) lldb_arch="x86_64" ;;
  * )
    echo "Unsupported ABI '$abi' from device." >&2
    exit 1
    ;;
esac

ndk_root="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
if [ -z "$ndk_root" ]; then
  sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
  if [ -n "$sdk_root" ]; then
    if [ -d "$sdk_root/ndk-bundle" ]; then
      ndk_root="$sdk_root/ndk-bundle"
    elif [ -d "$sdk_root/ndk" ]; then
      ndk_root="$(ls -d "$sdk_root/ndk/"* 2>/dev/null | sort -V | tail -n 1)"
    fi
  fi
fi

if [ -z "$ndk_root" ] || [ ! -d "$ndk_root" ]; then
  echo "Android NDK not found. Set ANDROID_NDK_ROOT or ANDROID_NDK_HOME." >&2
  exit 1
fi

host_os="$(uname -s)"
host_arch="$(uname -m)"
case "$host_os" in
  Darwin)
    if [ "$host_arch" = "arm64" ]; then
      host_tag="darwin-arm64"
    else
      host_tag="darwin-x86_64"
    fi
    ;;
  Linux) host_tag="linux-x86_64" ;;
  *) echo "Unsupported host OS: $host_os" >&2; exit 1 ;;
esac

toolchain_dir="$ndk_root/toolchains/llvm/prebuilt/$host_tag"
lldb_server=""

for clang_root in "$toolchain_dir/lib/clang" "$toolchain_dir/lib64/clang"; do
  if [ -d "$clang_root" ]; then
    while IFS= read -r ver; do
      candidate="$clang_root/$ver/lib/linux/$lldb_arch/lldb-server"
      if [ -x "$candidate" ]; then
        lldb_server="$candidate"
      fi
    done < <(ls "$clang_root" 2>/dev/null | sort -V)
  fi
done

if [ -z "$lldb_server" ]; then
  for prebuilt_dir in "$ndk_root/toolchains/llvm/prebuilt/"*; do
    if [ -d "$prebuilt_dir" ]; then
      for clang_root in "$prebuilt_dir/lib/clang" "$prebuilt_dir/lib64/clang"; do
        if [ -d "$clang_root" ]; then
          while IFS= read -r ver; do
            candidate="$clang_root/$ver/lib/linux/$lldb_arch/lldb-server"
            if [ -x "$candidate" ]; then
              lldb_server="$candidate"
            fi
          done < <(ls "$clang_root" 2>/dev/null | sort -V)
        fi
      done
    fi
  done
fi

if [ -z "$lldb_server" ]; then
  echo "Could not locate lldb-server for ABI '$abi' under $toolchain_dir" >&2
  exit 1
fi

tmp_server="/data/local/tmp/android-debug/lldb-server"
dest_dir="/data/data/$PACKAGE_NAME/android-debug/lldb/bin"
dest_server="$dest_dir/lldb-server"

echo "Pushing lldb-server..."
"${adb_cmd[@]}" push "$lldb_server" "$tmp_server" >/dev/null
"${adb_cmd[@]}" shell "run-as $PACKAGE_NAME mkdir -p $dest_dir" >/dev/null
"${adb_cmd[@]}" shell "cat $tmp_server | run-as $PACKAGE_NAME sh -c 'cat > $dest_server && chmod 700 $dest_server'" >/dev/null

echo "Forwarding tcp:$PORT..."
"${adb_cmd[@]}" forward --remove "tcp:$PORT" >/dev/null 2>&1 || true
"${adb_cmd[@]}" forward "tcp:$PORT" "tcp:$PORT"

echo "Launching app activity..."
"${adb_cmd[@]}" shell "am start -a android.intent.action.MAIN -c android.intent.category.LAUNCHER $PACKAGE_NAME/$LAUNCH_ACTIVITY" >/dev/null

sleep 1
pid_out="$("${adb_cmd[@]}" shell pidof "$PACKAGE_NAME" 2>/dev/null || true)"
pid="$(echo "$pid_out" | tr -d '\r' | awk '{print $1}')"
if [ -z "$pid" ]; then
  echo "Could not find running pid for $PACKAGE_NAME." >&2
  exit 1
fi

log_path="zed-lldb-server.log"
start_cmd="$dest_server gdbserver --listen tcp:$PORT --attach $pid"

echo "Starting lldb-server..."
"${adb_cmd[@]}" shell "run-as $PACKAGE_NAME sh -c '$start_cmd >$log_path 2>&1 < /dev/null &'" >/dev/null

sleep 1
echo "lldb-server listening on tcp:$PORT (adb forward active)."
