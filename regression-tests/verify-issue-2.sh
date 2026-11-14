#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "== [Verify Issue 2] Building rea-movie =="
cargo build

BIN="$REPO_ROOT/target/debug/rea-movie"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

SAMPLE_VIDEO="$TMP_DIR/movie.mp4"
PROJ_DIR="$TMP_DIR/movie_proj"

ffmpeg -v error -y \
  -f lavfi -i "color=c=green:s=640x360:r=24:d=3" \
  -f lavfi -i "color=c=yellow:s=640x360:r=24:d=3" \
  -f lavfi -i "sine=frequency=520:sample_rate=32000:duration=6" \
  -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" \
  -map "[v]" -map 2:a -c:v libx264 -pix_fmt yuv420p -c:a aac "$SAMPLE_VIDEO"

"$BIN" "$SAMPLE_VIDEO" -o "$PROJ_DIR" --offline

export REA_MOVIE_MOCK_H3=1

echo "== [Verify Issue 2] 1. Smoke render isolation test (608x352) =="
(cd "$PROJ_DIR" && "$BIN" render --smoke)
SMOKE_MP4="$PROJ_DIR/shots/shot_0001/renders/smoke/shot_0001.mp4"
PROD_MP4="$PROJ_DIR/shots/shot_0001/renders/prod/shot_0001.mp4"
[ -f "$SMOKE_MP4" ] || { echo "FAIL: smoke mp4 not created at $SMOKE_MP4"; exit 1; }
[ ! -f "$PROD_MP4" ] || { echo "FAIL: prod mp4 should not exist yet"; exit 1; }

SMOKE_W=$(ffprobe -v error -select_streams v:0 -show_entries stream=width -of default=nw=1:nk=1 "$SMOKE_MP4")
[ "$SMOKE_W" = "608" ] || { echo "FAIL: expected smoke width 608, got $SMOKE_W"; exit 1; }

echo "== [Verify Issue 2] 2. Default Prod render test (960x576) =="
(cd "$PROJ_DIR" && "$BIN" render)
[ -f "$PROD_MP4" ] || { echo "FAIL: prod mp4 not created at $PROD_MP4"; exit 1; }
PROD_W=$(ffprobe -v error -select_streams v:0 -show_entries stream=width -of default=nw=1:nk=1 "$PROD_MP4")
[ "$PROD_W" = "960" ] || { echo "FAIL: expected prod width 960, got $PROD_W"; exit 1; }

echo "== [Verify Issue 2] 3. Skip-by-default & --force backup history test =="
MTIME_BEFORE=$(stat -c %Y "$PROD_MP4")
sleep 1
(cd "$PROJ_DIR" && "$BIN" render --shot 0001)
MTIME_AFTER=$(stat -c %Y "$PROD_MP4")
[ "$MTIME_BEFORE" = "$MTIME_AFTER" ] || { echo "FAIL: existing prod mp4 was NOT skipped"; exit 1; }

echo "새로운 한국어 연출 수정 테스트 문구" >> "$PROJ_DIR/shots/shot_0001/prompt.ko.md"
(cd "$PROJ_DIR" && "$BIN" render --shot 0001 --force --offline)
[ -f "$PROJ_DIR/shots/shot_0001/renders/prod/history/take_001.mp4" ] || { echo "FAIL: history/take_001.mp4 backup not created on --force"; exit 1; }

echo "== [Verify Issue 2] ALL CHECKS PASSED =="
