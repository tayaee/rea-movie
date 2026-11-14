#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "== [Verify Issue 3] Building rea-movie =="
cargo build

BIN="$REPO_ROOT/target/debug/rea-movie"
export PATH="$REPO_ROOT/target/debug:$PATH"
export REA_MOVIE_MOCK_H3=1

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

SAMPLE_VIDEO="$TMP_DIR/robocop.mp4"
PROJ_DIR="$TMP_DIR/robocop"

ffmpeg -v error -y \
  -f lavfi -i "color=c=red:s=640x360:r=24:d=3" \
  -f lavfi -i "color=c=blue:s=640x360:r=24:d=3" \
  -f lavfi -i "sine=frequency=440:sample_rate=32000:duration=6" \
  -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" \
  -map "[v]" -map 2:a -c:v libx264 -pix_fmt yuv420p -c:a aac "$SAMPLE_VIDEO"

echo "== [Verify Issue 3] 1. Decompile and run ./run.sh (Prod default) =="
"$BIN" "$SAMPLE_VIDEO" -o "$PROJ_DIR" --offline
(cd "$PROJ_DIR" && ./run.sh)

[ -f "$PROJ_DIR/outputs/prod/robocop_prod.mp4" ] || { echo "FAIL: outputs/prod/robocop_prod.mp4 missing"; exit 1; }
[ -f "$PROJ_DIR/outputs/prod/robocop_compare.mp4" ] || { echo "FAIL: outputs/prod/robocop_compare.mp4 missing"; exit 1; }

echo "== [Verify Issue 3] 2. Run ./run.sh --smoke and verify separate outputs/smoke/ =="
(cd "$PROJ_DIR" && ./run.sh --smoke)
[ -f "$PROJ_DIR/outputs/smoke/robocop_smoke.mp4" ] || { echo "FAIL: outputs/smoke/robocop_smoke.mp4 missing"; exit 1; }
[ -f "$PROJ_DIR/outputs/smoke/robocop_compare.mp4" ] || { echo "FAIL: outputs/smoke/robocop_compare.mp4 missing"; exit 1; }

echo "== [Verify Issue 3] 3. Partial shot regeneration via ./run.sh 0001 --force =="
(cd "$PROJ_DIR" && ./run.sh 0001 --force)
[ -f "$PROJ_DIR/shots/shot_0001/renders/prod/history/take_001.mp4" ] || { echo "FAIL: history backup missing after ./run.sh 0001 --force"; exit 1; }

echo "== [Verify Issue 3] 4. Verify MCP server tools/list =="
MCP_OUT=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n' | "$BIN" mcp)
echo "$MCP_OUT" | grep -q "decompile_movie" || { echo "FAIL: MCP tools/list missing decompile_movie"; exit 1; }
echo "$MCP_OUT" | grep -q "render_shot" || { echo "FAIL: MCP tools/list missing render_shot"; exit 1; }
echo "$MCP_OUT" | grep -q "assemble_movie" || { echo "FAIL: MCP tools/list missing assemble_movie"; exit 1; }

echo "== [Verify Issue 3] 5. Verify /rea-movie skill file =="
[ -f "$REPO_ROOT/skills/rea-movie/SKILL.md" ] || { echo "FAIL: skills/rea-movie/SKILL.md missing"; exit 1; }

echo "== [Verify Issue 3] ALL CHECKS PASSED =="
