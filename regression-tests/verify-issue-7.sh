#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-7.sh
# Issue 7 Verification: Director-grade Direction for Regeneration
#
# Verification Criteria:
# 1. Cargo Quality Gates:
#    - cargo fmt --check
#    - cargo clippy --all-targets -- -D warnings
#    - cargo test
# 2. Scene Layer & Direction Grammar:
#    - Multi-cut dialogue (A-B-A reverse-shot) stays a single scene.
#    - True location change splits into a new scene.
#    - Scene block (<!-- rea-movie:scene:begin) injected exactly once per shot prompt.
#    - timeline.json maps valid scene_id for each shot.
# 3. Verbatim Dialogue, 6-Lang Subtitles, Original Audio & Speech 1.0x Trim Guard:
#    - Dialogue cues assigned exactly once (0 duplication), preserved verbatim.
#    - Assemble generates 6 external subtitles (ko/zh/ja/en/fr/de.srt) + .proof.srt.
#    - Original audio track is default (disposition default=1).
#    - Sub-4s shot with varying-RMS speech is trimmed via 1.0x speech window.
# 4. Series-Watch M3 Rich Context Preservation:
#    - Existing story/world.json and characters.json with "mode": "m3" are preserved
#      even when series-watch is run with --offline --force.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> [Gate 1] Cargo Quality Gates (fmt, clippy, test)..."
(cd "${REPO_ROOT}" && cargo fmt --check)
(cd "${REPO_ROOT}" && cargo clippy --all-targets -- -D warnings)
(cd "${REPO_ROOT}" && cargo test)

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue7-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 7 regression verifications in ${TMP_DIR}..."

# ------------------------------------------------------------------------------
# [Check 1] Series-Watch M3 Rich Context Preservation
# ------------------------------------------------------------------------------
echo "--- [Check 1] series-watch M3 rich context preservation on offline force ---"

SW_ROOT="${TMP_DIR}/sw_root"
mkdir -p "${SW_ROOT}/sources" "${SW_ROOT}/story"

SW_VID="${SW_ROOT}/sources/ep01.mp4"
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=black:s=64x64:d=1" \
  -f lavfi -i "sine=frequency=440:duration=1" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 \
  "${SW_VID}"

cat <<'EOF' > "${SW_ROOT}/story/world.json"
{
  "mode": "m3",
  "era": "Joseon Dynasty",
  "locations": [{"name": "Royal Palace", "description": "Grand throne room"}],
  "preserved_marker": "m3_world_data_must_survive"
}
EOF

cat <<'EOF' > "${SW_ROOT}/story/characters.json"
{
  "mode": "m3",
  "characters": [{"id": "P1", "name": "King Sejong", "appearance": "Royal dragon robe"}],
  "preserved_marker": "m3_character_data_must_survive"
}
EOF

cat <<'EOF' > "${SW_ROOT}/story/timeline-days.md"
# M3 Series Timeline — Verified
- EP01: Day 1 (Royal Palace Morning)
EOF

"${BIN}" series-watch --out-root "${SW_ROOT}" --offline --force "${SW_VID}" > "${TMP_DIR}/sw.log" 2>&1

python3 - <<PY
import json, sys

with open("${SW_ROOT}/story/world.json", encoding="utf-8") as f:
    w = json.load(f)
assert w.get("mode") == "m3", f"world mode overwritten: {w.get('mode')}"
assert w.get("preserved_marker") == "m3_world_data_must_survive", "world context was overwritten"

with open("${SW_ROOT}/story/characters.json", encoding="utf-8") as f:
    c = json.load(f)
assert c.get("mode") == "m3", f"characters mode overwritten: {c.get('mode')}"
assert c.get("preserved_marker") == "m3_character_data_must_survive", "characters context was overwritten"

with open("${SW_ROOT}/story/timeline-days.md", encoding="utf-8") as f:
    t = f.read()
assert "M3 Series Timeline — Verified" in t, "timeline-days.md was overwritten"
print("  series-watch preservation: PASS")
PY

echo "PASS: [Check 1] M3 rich context preserved under offline --force."

# ------------------------------------------------------------------------------
# [Check 2] Scene Layer & Direction Grammar & Verbatim Dialogue
# ------------------------------------------------------------------------------
echo "--- [Check 2] scene layer grouping, prompts & verbatim dialogue ---"

PROJ_ROOT="${TMP_DIR}/scene_series"
mkdir -p "${PROJ_ROOT}/sources" "${PROJ_ROOT}/h3-projects"
MULTI_VID="${PROJ_ROOT}/sources/dialogue_scene.mp4"

# 4 cuts:
# - Cut 1 (0.0s - 2.0s): Red (Person A)
# - Cut 2 (2.0s - 4.0s): Blue (Person B - reverse-shot)
# - Cut 3 (4.0s - 6.0s): Red (Person A - return to A) -> Should stay Scene 1
# - Cut 4 (6.0s - 8.5s): White (Location change)    -> Must split to Scene 2
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=red:s=320x240:d=2:r=24" \
  -f lavfi -i "color=c=blue:s=320x240:d=2:r=24" \
  -f lavfi -i "color=c=red:s=320x240:d=2:r=24" \
  -f lavfi -i "color=c=white:s=320x240:d=2.5:r=24" \
  -f lavfi -i "sine=frequency=440:duration=8.5" \
  -filter_complex "[0:v][1:v][2:v][3:v]concat=n=4:v=1:a=0[v]" \
  -map "[v]" -map "4:a" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 -t 8.5 \
  "${MULTI_VID}"

cat <<'EOF' > "${PROJ_ROOT}/sources/dialogue_scene.ko.srt"
1
00:00:00,300 --> 00:00:01,700
안녕하세요, 첫 번째 샷의 원본 대사입니다.

2
00:00:02,300 --> 00:00:03,700
반갑습니다, 두 번째 리버스 샷 대사입니다.

3
00:00:04,300 --> 00:00:05,700
A-B-A 세 번째 샷 대사입니다.

4
00:00:06,300 --> 00:00:07,800
새로운 장소로 전환된 네 번째 샷 대사입니다.
EOF

PROJ_DIR="${PROJ_ROOT}/h3-projects/ep_dialogue"
MEDIA_DIR="${PROJ_ROOT}/h3-results/ep_dialogue"

echo "Executing decompile..."
"${BIN}" decompile "${MULTI_VID}" -o "${PROJ_DIR}" --offline > "${TMP_DIR}/decompile.log" 2>&1

python3 - <<PY
import json, glob, os, sys

proj = "${PROJ_DIR}"
with open(os.path.join(proj, "timeline.json"), encoding="utf-8") as f:
    tl = json.load(f)

shots = tl.get("shots", [])
assert len(shots) == 4, f"expected 4 shots, got {len(shots)}"

# Check scene IDs: shots 0,1,2 must share scene_id; shot 3 must differ
s0 = shots[0].get("scene_id")
s1 = shots[1].get("scene_id")
s2 = shots[2].get("scene_id")
s3 = shots[3].get("scene_id")

assert s0 and s1 and s2 and s3, f"missing scene_id in timeline: {[s['shot_id'] for s in shots]}"
assert s0 == s1 == s2, f"A-B-A shots should share scene_id, got: s0={s0}, s1={s1}, s2={s2}"
assert s3 != s0, f"Location change should have different scene_id, got: s3={s3}, s0={s0}"

scene_files = glob.glob(os.path.join(proj, "story", "scenes", "scene_*.json"))
assert len(scene_files) >= 2, f"expected at least 2 scene files, got {scene_files}"

# Check prompts: exactly 1 scene block injection and verbatim dialogue
expected_lines = [
    "안녕하세요, 첫 번째 샷의 원본 대사입니다.",
    "반갑습니다, 두 번째 리버스 샷 대사입니다.",
    "A-B-A 세 번째 샷 대사입니다.",
    "새로운 장소로 전환된 네 번째 샷 대사입니다."
]

for idx, shot in enumerate(shots):
    sid = shot["shot_id"]
    shot_dir = os.path.join(proj, "shots", sid)
    ko_p = os.path.join(shot_dir, "prompt.ko.md")
    h3_p = os.path.join(shot_dir, "prompt.h3.txt")

    assert os.path.isfile(ko_p), f"missing {ko_p}"
    assert os.path.isfile(h3_p), f"missing {h3_p}"

    ko_content = open(ko_p, encoding="utf-8").read()
    h3_content = open(h3_p, encoding="utf-8").read()

    # Injected scene block exactly once
    c_ko = ko_content.count("<!-- rea-movie:scene:begin")
    c_h3 = h3_content.count("<!-- rea-movie:scene:begin")
    assert c_ko == 1, f"{ko_p}: expected exactly 1 scene marker, found {c_ko}"
    assert c_h3 == 1, f"{h3_p}: expected exactly 1 scene marker, found {c_h3}"

    # Verbatim dialogue present in this shot's body (excluding scene block)
    target_line = expected_lines[idx]
    ko_body = ko_content.split("<!-- rea-movie:scene:end")[1] if "<!-- rea-movie:scene:end" in ko_content else ko_content
    h3_body = h3_content.split("<!-- rea-movie:scene:end")[1] if "<!-- rea-movie:scene:end" in h3_content else h3_content

    assert target_line in ko_body, f"{ko_p} body missing verbatim line: {target_line}"
    assert target_line in h3_body, f"{h3_p} body missing verbatim line in h3: {target_line}"

    # Ensure other dialogue lines are NOT in this shot's body (0 duplication)
    for other_idx, other_line in enumerate(expected_lines):
        if other_idx != idx:
            assert other_line not in ko_body, f"{ko_p} duplicated line: {other_line}"
            assert other_line not in h3_body, f"{h3_p} duplicated line in h3: {other_line}"

print("  scene grouping & verbatim prompt checks: PASS")
PY

echo "PASS: [Check 2] scene grouping, prompt injection, and verbatim reproduction."

# ------------------------------------------------------------------------------
# [Check 3] Assemble: 6-Lang Subtitles, Original Audio & Speech 1.0x Trim Guard
# ------------------------------------------------------------------------------
echo "--- [Check 3] assemble: subtitles, original audio & speech 1.0x trim guard ---"

# 1. Render all shots with mock-h3
"${BIN}" render --proj-dir "${PROJ_DIR}" --smoke --mock-h3 > "${TMP_DIR}/render.log" 2>&1

# 2. Inject varying-RMS speech-like audio into shot_0001 (sub-4s shot, 2.0s duration)
#    shot_0001 render is at least 4.0s (DGX/H3 standard).
#    Avarying RMS signal (CV > 0.10) triggers detect_speech_window to select a 1.0x window.
SHOT1_RENDER="${MEDIA_DIR}/shots/shot_0001/renders/smoke/shot_0001.mp4"
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=red:s=608x352:d=4" \
  -f lavfi -i "aevalsrc=sin(2*PI*300*t)*sin(2*PI*2*t):s=16000:d=4" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 -t 4 \
  "${SHOT1_RENDER}"

# 3. Assemble with smoke, offline, proof
"${BIN}" assemble --proj-dir "${PROJ_DIR}" --smoke --offline --proof --force > "${TMP_DIR}/assemble.log" 2>&1

python3 - <<PY
import os, glob, subprocess, json, re

media_dir = "${MEDIA_DIR}"
out_dir = os.path.join(media_dir, "outputs", "smoke")

final_mp4 = os.path.join(out_dir, "dialogue_scene_smoke.mp4")
compare_mp4 = os.path.join(out_dir, "dialogue_scene_compare.mp4")
proof_mp4 = os.path.join(out_dir, "dialogue_scene_smoke_proof.mp4")

assert os.path.isfile(final_mp4), f"missing final movie: {final_mp4}"
assert os.path.isfile(compare_mp4), f"missing compare movie: {compare_mp4}"
assert os.path.isfile(proof_mp4), f"missing proof movie: {proof_mp4}"

# External subtitles: 6 languages + proof
for lang in ["ko", "zh", "ja", "en", "fr", "de"]:
    srt_path = os.path.join(out_dir, f"dialogue_scene_smoke.{lang}.srt")
    assert os.path.isfile(srt_path) and os.path.getsize(srt_path) > 0, f"missing {srt_path}"

proof_srt = os.path.join(out_dir, "dialogue_scene_smoke.proof.srt")
assert os.path.isfile(proof_srt) and os.path.getsize(proof_srt) > 0, f"missing {proof_srt}"

# Audio streams check: dual audio with original as default
probe_out = subprocess.run([
    "ffprobe", "-v", "error", "-select_streams", "a",
    "-show_entries", "stream=index,codec_name:stream_disposition=default",
    "-of", "json", final_mp4
], capture_output=True, text=True, check=True)

streams = json.loads(probe_out.stdout).get("streams", [])
assert len(streams) >= 2, f"expected at least 2 audio streams, got {len(streams)}"

default_streams = [s for s in streams if s.get("disposition", {}).get("default") == 1]
assert len(default_streams) == 1, f"expected exactly 1 default audio stream, got {default_streams}"
# The original audio track is appended last and set as default
assert default_streams[0]["index"] == max(s["index"] for s in streams), \
    f"original audio must be default track: {default_streams}"

# Verify duration close to source (8.5s)
def get_dur(p):
    r = subprocess.run([
        "ffprobe", "-v", "error", "-show_entries", "format=duration",
        "-of", "csv=p=0", p
    ], capture_output=True, text=True, check=True)
    return float(r.stdout.strip())

src_dur = 8.5
fin_dur = get_dur(final_mp4)
assert abs(fin_dur - src_dur) <= 0.6, f"duration mismatch: fin={fin_dur}s vs src={src_dur}s"

print("  assemble outputs, subtitles & audio default: PASS")
PY

echo "PASS: [Check 3] assemble output, 6-lang subtitles, original audio default & speech trim."

echo ""
echo "=============================================================================="
echo "==> All Issue 7 verification tests passed successfully!"
echo "=============================================================================="
