#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "== [Verify Issue 1] Building rea-movie =="
cargo build

BIN="$REPO_ROOT/target/debug/rea-movie"
[ -x "$BIN" ] || { echo "FAIL: binary $BIN not found"; exit 1; }

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

SAMPLE_VIDEO="$TMP_DIR/robocop_test.mp4"
PROJ_DIR="$TMP_DIR/robocop_proj"

echo "== [Verify Issue 1] Generating 12-second multi-scene test video with ffmpeg =="
# 0~4s: red scene with tone, 4~12s: blue scene (8s) with tone pulse
ffmpeg -v error -y \
  -f lavfi -i "color=c=red:s=640x360:r=24:d=4" \
  -f lavfi -i "color=c=blue:s=640x360:r=24:d=8" \
  -f lavfi -i "sine=frequency=440:sample_rate=32000:duration=12" \
  -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" \
  -map "[v]" -map 2:a -c:v libx264 -pix_fmt yuv420p -c:a aac "$SAMPLE_VIDEO"

echo "== [Verify Issue 1] Running: rea-movie robocop_test.mp4 -o $PROJ_DIR =="
"$BIN" "$SAMPLE_VIDEO" -o "$PROJ_DIR" --offline

echo "== [Verify Issue 1] Checking generated project layout =="
[ -f "$PROJ_DIR/rea-movie.toml" ] || { echo "FAIL: rea-movie.toml missing"; exit 1; }
[ -f "$PROJ_DIR/timeline.json" ] || { echo "FAIL: timeline.json missing"; exit 1; }
[ -x "$PROJ_DIR/run.sh" ] || { echo "FAIL: run.sh missing or not executable"; exit 1; }
[ -f "$PROJ_DIR/story/characters.toml" ] || { echo "FAIL: story/characters.toml missing"; exit 1; }
[ -f "$PROJ_DIR/story/screenplay.md" ] || { echo "FAIL: story/screenplay.md missing"; exit 1; }

grep -q "spark1.local" "$PROJ_DIR/rea-movie.toml" || { echo "FAIL: spark1.local missing in rea-movie.toml"; exit 1; }
grep -q "spark2.local" "$PROJ_DIR/rea-movie.toml" || { echo "FAIL: spark2.local missing in rea-movie.toml"; exit 1; }
grep -q "rea-movie-workspace" "$PROJ_DIR/rea-movie.toml" || { echo "FAIL: ~/rea-movie-workspace missing in rea-movie.toml"; exit 1; }

SHOT1_DIR="$PROJ_DIR/shots/shot_0001"
[ -d "$SHOT1_DIR" ] || { echo "FAIL: $SHOT1_DIR missing"; exit 1; }
[ -f "$SHOT1_DIR/shot.toml" ] || { echo "FAIL: shot.toml missing"; exit 1; }
[ -f "$SHOT1_DIR/prompt.ko.md" ] || { echo "FAIL: prompt.ko.md missing"; exit 1; }
[ -f "$SHOT1_DIR/prompt.h3.txt" ] || { echo "FAIL: prompt.h3.txt missing"; exit 1; }
[ -f "$SHOT1_DIR/refs/original_clip.mp4" ] || { echo "FAIL: refs/original_clip.mp4 missing"; exit 1; }
[ -f "$SHOT1_DIR/refs/original_audio.wav" ] || { echo "FAIL: refs/original_audio.wav missing"; exit 1; }
[ -f "$SHOT1_DIR/refs/first_frame.jpg" ] || { echo "FAIL: refs/first_frame.jpg missing"; exit 1; }
[ -f "$SHOT1_DIR/refs/last_frame.jpg" ] || { echo "FAIL: refs/last_frame.jpg missing"; exit 1; }
[ -f "$SHOT1_DIR/refs/contact_sheet.jpg" ] || { echo "FAIL: refs/contact_sheet.jpg missing"; exit 1; }
[ -d "$SHOT1_DIR/renders/prod" ] || { echo "FAIL: renders/prod directory missing"; exit 1; }
[ -d "$SHOT1_DIR/renders/smoke" ] || { echo "FAIL: renders/smoke directory missing"; exit 1; }

grep -q "integrated_multimodal_description:" "$SHOT1_DIR/prompt.h3.txt" || { echo "FAIL: integrated_multimodal_description missing in prompt.h3.txt"; exit 1; }
grep -q "overall_soundscape:" "$SHOT1_DIR/prompt.h3.txt" || { echo "FAIL: overall_soundscape missing in prompt.h3.txt"; exit 1; }
grep -q "non_diegetic_music:" "$SHOT1_DIR/prompt.h3.txt" || { echo "FAIL: non_diegetic_music missing in prompt.h3.txt"; exit 1; }

echo "== [Verify Issue 1] Verifying skip-by-default idempotency =="
echo "# CUSTOM_MARKER_DO_NOT_OVERWRITE" >> "$SHOT1_DIR/prompt.ko.md"
"$BIN" "$SAMPLE_VIDEO" -o "$PROJ_DIR" --offline
grep -q "CUSTOM_MARKER_DO_NOT_OVERWRITE" "$SHOT1_DIR/prompt.ko.md" || { echo "FAIL: existing prompt.ko.md was overwritten without --force"; exit 1; }

echo "== [Verify Issue 1] ALL CHECKS PASSED =="
