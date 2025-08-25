#!/bin/bash

# Build script for StateDigest JNI library
# This script compiles the Rust library and copies it to the appropriate location

set -e

echo "Building StateDigest JNI library..."

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "Error: Rust/Cargo is not installed. Please install Rust from https://rustup.rs/"
    exit 1
fi

# Build the library in release mode
echo "Compiling Rust library..."
cargo build --release

# Determine the library file name based on the platform
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
    Linux*)
        LIB_NAME="libstate_digest_jni.so"
        TARGET_DIR="linux"
        ;;
    Darwin*)
        LIB_NAME="libstate_digest_jni.dylib"
        TARGET_DIR="mac"
        ;;
    CYGWIN*|MINGW*|MSYS*)
        LIB_NAME="state_digest_jni.dll"
        TARGET_DIR="windows"
        ;;
    *)
        echo "Error: Unsupported operating system: $OS"
        exit 1
        ;;
esac

# Normalize architecture name
case "$ARCH" in
    x86_64|amd64)
        ARCH_DIR="x86_64"
        ;;
    aarch64|arm64)
        ARCH_DIR="aarch64"
        ;;
    *)
        echo "Warning: Unknown architecture: $ARCH, using as-is"
        ARCH_DIR="$ARCH"
        ;;
esac

# Create target directory structure
RESOURCES_DIR="../framework/src/main/resources/native/$TARGET_DIR/$ARCH_DIR"
mkdir -p "$RESOURCES_DIR"

# Copy the library to the resources directory
echo "Copying library to $RESOURCES_DIR/$LIB_NAME"
cp "target/release/$LIB_NAME" "$RESOURCES_DIR/"

# Also copy to a common location for development
DEV_LIB_DIR="../framework/native-libs"
mkdir -p "$DEV_LIB_DIR"
cp "target/release/$LIB_NAME" "$DEV_LIB_DIR/"

echo "Build completed successfully!"
echo "Library location: $RESOURCES_DIR/$LIB_NAME"
echo "Development copy: $DEV_LIB_DIR/$LIB_NAME"

# Run tests
echo "Running Rust tests..."
cargo test

echo "StateDigest JNI library build complete!"
