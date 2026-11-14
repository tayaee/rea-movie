#!/bin/sh
# 4-commit.sh — commit staged changes.
# Usage: ./4-commit.sh [<message>]  (omitted: generate via ./cm.sh)
set -eu
# Cargo.lock is refreshed by cargo itself during gate/build — stage it so the
# verified build's lockfile is committed together with the code.
if [ -f Cargo.lock ] && ! git diff --quiet -- Cargo.lock 2>/dev/null; then git add Cargo.lock; fi
git diff --cached --quiet && { echo "[commit] nothing staged — run ./1-add.sh first" >&2; exit 1; }
if [ $# -ge 1 ]; then
    MSG="$1"
else
    MSG="$("$(dirname "$0")/cm.sh")"
fi
git commit -m "$MSG"
