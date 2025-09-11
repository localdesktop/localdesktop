# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Local Desktop is a Rust rewrite of the Polar Bear project that enables running a desktop Linux environment on Android devices. The app creates an Arch Linux ARM64 filesystem in internal storage, uses Proot for a chroot-like environment, runs a minimal Wayland compositor in Android NDK, and launches Xwayland with a desktop environment that renders back to the Android native activity.

## Build Commands

### Primary Build Commands
- **Release APK**: `x build --release --platform android --arch arm64 --format apk`
- **Debug APK**: `x build --platform android --arch arm64 --format apk`
- **Install xbuild tool**: `cargo install xbuild`

### VS Code Integration
- **Default build task**: `Ctrl+Shift+B` (configured in `.vscode/tasks.json`)
- **Debug configuration**: `[Android] Debug` from VS Code debug panel

### Output Locations
- Release APK: `target/x/release/android/gradle/app/build/outputs/apk/debug/`
- Debug APK: `target/x/debug/android/gradle/app/build/outputs/apk/debug/app-debug.apk`

## Development Setup

### Required Tools
- **xbuild**: Cross-platform build tool (`cargo install xbuild`)
- **Android SDK/NDK**: For Android development
- **adb**: For device debugging and logcat

### Recommended VS Code Extensions
- Rust Analyzer
- Android Debug
- CodeLLDB

### Development Workflow
1. Use `[Android] Debug` configuration to build, install, and debug on device
2. Monitor logs with: `adb logcat -c && adb logcat -s RustStdoutStderr`
3. Physical or virtual Android devices supported for debugging

## Architecture

### Core Structure
- **`src/android/`**: Android-specific code including Wayland backend, Proot integration, and app lifecycle
- **`src/core/`**: Cross-platform core functionality (logging, config)
- **`patches/`**: Patched dependencies (smithay, winit, xbuild) with local modifications
- **Wayland Compositor**: Built-in minimal compositor using Smithay framework
- **Proot Integration**: Manages Linux filesystem and chroot environment

### Key Components
- **Wayland Backend** (`src/android/backend/wayland/`): Compositor, input handling, window management
- **Proot Management** (`src/android/proot/`): Linux environment setup and process management
- **Android Utils** (`src/android/utils/`): NDK utilities, application context, webview integration

### Dependencies
- Uses patched versions of smithay (Wayland compositor) and winit (windowing)
- Targets ARM64 Android devices exclusively
- Optimized for size with `opt-level = "z"` and LTO enabled

## Agent Usage Guidelines

When working on this project, use the appropriate specialized agents:

### android-ndk-expert
Use for:
- Android NDK compilation and linking issues
- Manual APK/AAB packaging without Android Studio
- Maintaining code in `@patches/xbuild` directory
- Native library integration with Android
- Gradle build system issues with native components

### rust-cross-build-expert  
Use for:
- Cross-compilation targeting Android ARM64
- Cargo.toml configuration for Android targets
- Linker errors when building for Android ARM64
- Platform-specific compilation issues
- Setting up cross-compilation toolchains

### termux-proot-expert
Use for:
- Proot and proot-distro implementation guidance
- Wayland compositor optimization for Android
- Filesystem setup and initialization
- Android integration patterns from Termux ecosystem
- Performance optimization for Proot environments

## Development Guidelines

### File Creation Policy
- NEVER create example files (e.g., *_example.rs, *_sample.rs) - they add no value and clutter the codebase
- NEVER create files unless they're absolutely necessary for achieving the goal
- ALWAYS prefer editing an existing file to creating a new one
- NEVER proactively create documentation files (*.md) or README files unless explicitly requested

### Testing Strategy
- Write testable code outside `src/android/` when possible (not guarded by `#[cfg(target_os = "android")]`)
- Use `cargo test` for non-Android-dependent code
- Keep Android-dependent code in `src/android/` minimal for better modularity
- Structure code to maximize testability without Android dependencies