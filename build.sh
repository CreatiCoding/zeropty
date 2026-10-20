#!/bin/sh
# Builds into ./dist: the zeropty server and CLI, the shim and claude wrapper, and the embeddable engine
# (libzeropty_c.dylib + zeropty.h for any language, zeropty.node for Node), and the bash builtin.
set -e
cd "$(dirname "$0")"
cargo build --release --workspace
mkdir -p dist
# Replace, never overwrite in place: macOS caches a binary's signature per inode and SIGKILLs the rewritten file.
rm -f dist/zeropty dist/shim.dylib dist/libzeropty_c.dylib dist/zeropty.node dist/zeropty_bash.dylib
cp target/release/zeropty dist/
cp target/release/libzeropty_c.dylib dist/
cp target/release/libzeropty_node.dylib dist/zeropty.node
cp bindings/c/include/zeropty.h dist/
clang -dynamiclib -O2 -arch arm64 -arch arm64e -o dist/shim.dylib shim/shim.c
clang -dynamiclib -O2 -Wall -o dist/zeropty_bash.dylib shell/zeropty_bash.c
cp claude-zeropty zeropty-tty dist/ && chmod +x dist/claude-zeropty dist/zeropty-tty
echo "built: $(pwd)/dist"
