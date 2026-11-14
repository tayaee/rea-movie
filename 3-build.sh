#!/bin/sh
# 3-build.sh — local release verification build (real release is built by CI).
set -eu
cargo build --release
./target/release/rea-movie pool status
ls -lh target/release/rea-movie
echo "[build] ok"
