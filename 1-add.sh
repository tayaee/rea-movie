#!/bin/sh
# 1-add.sh — stage tracked updates; also stage new files if any appear.
# Only source is staged: heavy outputs (target/, outputs/, renders/,
# *.mp4/*.wav, workspace dirs) must be listed in .gitignore so they
# never appear in `git ls-files --others --exclude-standard`.
set -eu
git add -u
# NUL-safe staging: the old `echo "$NEW" | xargs git add --` split on
# spaces/quotes/newlines, so a file like `my clip.mp4` (or any name with
# blanks) was passed as two pathspecs and `git add` exited non-zero
# under `set -e`. `-z` + `-0` keeps each path intact; `-r` skips `git add`
# when there is nothing new to stage.
git ls-files --others --exclude-standard -z | xargs -0 -r git add --
echo "[add] staged:"
git diff --cached --name-only
