#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-10.sh
# Issue 10 Verification: pro director grammar — A-B-A reverse-shot grouping,
# on-screen vs off-screen delivery, 180-degree axis + camera grammar.
#
# Criteria:
# 1. (A) A-B-A-B cross-edit (1-hop 0.70, 2-hop 0.10) stays a single scene;
#    true location change (1/2/3-hop all 0.70) splits.
# 2. (B) Off-screen lines ((VO) tag, [S2] while on-screen is [S1],
#    delivery_mode='off_screen') produce 'no lip movement'/'off-screen' in
#    prompt.ko.md + prompt.h3.txt and NO 'synchronized lip movement'.
# 3. (C) SceneBible axis_of_action + camera_grammar serde + scene-block injection.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue10-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 10 regression verifications in ${TMP_DIR}..."

make_video() {
  local out="$1"
  local color="$2"
  ffmpeg -nostdin -v error -y \
    -f lavfi -i "color=c=${color}:s=320x240:d=4" \
    -f lavfi -i "sine=frequency=440:duration=4" \
    -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
    -c:a aac -ar 32000 -ac 2 -t 4 \
    "${out}"
}

# ------------------------------------------------------------------------------
# Check 1: A-B-A reverse-shot grouping (unit tests)
# ------------------------------------------------------------------------------
echo "--- [Check 1] A-B-A lookback grouping vs true location change ---"
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- \
  test_group_scenes_reverse_shot_no_split \
  test_group_scenes_true_location_change_splits \
  test_group_scenes_silence_still_splits_despite_lookback \
  2>&1 | tail -n 12)
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- \
  test_group_scenes_reverse_shot_no_split \
  test_group_scenes_true_location_change_splits \
  test_group_scenes_silence_still_splits_despite_lookback \
  2>&1 | grep -q "3 passed")
echo "PASS: [Check 1] reverse-shot stays single scene, true move splits, silence respected."

# ------------------------------------------------------------------------------
# Check 2: off-screen delivery end-to-end (offline decompile)
# ------------------------------------------------------------------------------
echo "--- [Check 2] off-screen vs on-screen prompt generation ---"

# 2a. (VO) tag -> off-screen
VID_VO="${TMP_DIR}/vo.mp4"
make_video "${VID_VO}" "blue"
cat > "${TMP_DIR}/vo.ko.srt" <<'EOF'
1
00:00:00,500 --> 00:00:02,000
(VO) 밖에 비가 내린다.

EOF
"${BIN}" decompile "${VID_VO}" -o "${TMP_DIR}/proj_vo" --offline >/dev/null
KO_VO="${TMP_DIR}/proj_vo/shots/shot_0001/prompt.ko.md"
H3_VO="${TMP_DIR}/proj_vo/shots/shot_0001/prompt.h3.txt"
[[ -f "${KO_VO}" && -f "${H3_VO}" ]] || { echo "FAIL: VO prompts missing"; exit 1; }
grep -q "화면 밖" "${KO_VO}" || { echo "FAIL: VO ko missing 화면 밖"; cat "${KO_VO}"; exit 1; }
grep -q "입을 다문" "${KO_VO}" || { echo "FAIL: VO ko missing 입을 다문"; cat "${KO_VO}"; exit 1; }
grep -q "no lip movement" "${H3_VO}" || { echo "FAIL: VO h3 missing no lip movement"; cat "${H3_VO}"; exit 1; }
grep -q "off-screen" "${H3_VO}" || { echo "FAIL: VO h3 missing off-screen"; cat "${H3_VO}"; exit 1; }
if grep -q "synchronized lip movement" "${H3_VO}"; then echo "FAIL: VO h3 must NOT contain synchronized lip movement"; exit 1; fi
echo "  [2a] (VO) off-screen: PASS"

# 2b. [S2] line while on-screen cast is [S1] -> off-screen
VID_S2="${TMP_DIR}/s2.mp4"
make_video "${VID_S2}" "green"
cat > "${TMP_DIR}/s2.ko.srt" <<'EOF'
1
00:00:00,500 --> 00:00:02,000
[S2] 이쪽으로 와.

EOF
"${BIN}" decompile "${VID_S2}" -o "${TMP_DIR}/proj_s2" --offline >/dev/null
H3_S2="${TMP_DIR}/proj_s2/shots/shot_0001/prompt.h3.txt"
grep -q "no lip movement" "${H3_S2}" || { echo "FAIL: [S2] h3 missing no lip movement"; cat "${H3_S2}"; exit 1; }
if grep -q "synchronized lip movement" "${H3_S2}"; then echo "FAIL: [S2] h3 must NOT lip-sync"; exit 1; fi
echo "  [2b] [S2]-while-[S1]-visible off-screen: PASS"

# 2c. plain on-screen control -> synchronized lip movement, no off-screen
VID_ON="${TMP_DIR}/on.mp4"
make_video "${VID_ON}" "red"
cat > "${TMP_DIR}/on.ko.srt" <<'EOF'
1
00:00:00,500 --> 00:00:02,000
안녕하세요. 반갑습니다.

EOF
"${BIN}" decompile "${VID_ON}" -o "${TMP_DIR}/proj_on" --offline >/dev/null
H3_ON="${TMP_DIR}/proj_on/shots/shot_0001/prompt.h3.txt"
grep -q "synchronized lip movement" "${H3_ON}" || { echo "FAIL: on-screen h3 missing lip sync"; cat "${H3_ON}"; exit 1; }
if grep -q "no lip movement" "${H3_ON}"; then echo "FAIL: on-screen h3 must NOT contain no lip movement"; exit 1; fi
echo "  [2c] on-screen lip sync: PASS"

# 2d. delivery_mode='off_screen' forces off-screen even for plain lines
PROJ_DM="${TMP_DIR}/proj_on"
SHOT_TOML="${PROJ_DM}/shots/shot_0001/shot.toml"
python3 - "${SHOT_TOML}" <<'PY'
import sys
p = sys.argv[1]
s = open(p, encoding="utf-8").read()
if "delivery_mode" not in s:
    s = s.rstrip() + '\ndelivery_mode = "off_screen"\n'
else:
    import re
    s = re.sub(r'delivery_mode\s*=.*', 'delivery_mode = "off_screen"', s)
open(p, "w", encoding="utf-8").write(s)
PY
rm -f "${PROJ_DM}/shots/shot_0001/prompt.ko.md" "${PROJ_DM}/shots/shot_0001/prompt.h3.txt"
"${BIN}" decompile "${VID_ON}" -o "${PROJ_DM}" --offline >/dev/null
H3_DM="${PROJ_DM}/shots/shot_0001/prompt.h3.txt"
grep -q "no lip movement" "${H3_DM}" || { echo "FAIL: delivery_mode h3 missing no lip movement"; cat "${H3_DM}"; exit 1; }
if grep -q "synchronized lip movement" "${H3_DM}"; then echo "FAIL: delivery_mode h3 must NOT lip-sync"; exit 1; fi
echo "  [2d] delivery_mode=off_screen: PASS"

# 2e. unit-level infer_delivery_mode coverage
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- \
  test_infer_delivery_mode_markers_and_hints \
  test_build_dialogue_off_screen_has_no_lipsync \
  2>&1 | grep -q "2 passed")
echo "PASS: [Check 2] off-screen prompts block lip-sync misdirection."

# ------------------------------------------------------------------------------
# Check 3: axis_of_action + camera_grammar bible + block injection
# ------------------------------------------------------------------------------
echo "--- [Check 3] 180-degree axis + camera grammar ---"
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- \
  test_bible_axis_fields_roundtrip_and_blocks test_bible_serde_roundtrip \
  2>&1 | tail -n 6)
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- test_bible_axis_fields_roundtrip_and_blocks 2>&1 | grep -q "1 passed")

PROJ_AX="${TMP_DIR}/proj_axis"
mkdir -p "${PROJ_AX}/shots/shot_0001" "${PROJ_AX}/story/scenes"
cat > "${PROJ_AX}/timeline.json" <<'EOF'
{"shots": [{"shot_id": "shot_0001", "start_sec": 0.0, "end_sec": 2.0, "duration": 2.0, "is_chained_longtake": false, "anchor": "first_and_last", "assignee": "", "characters": ["S1"], "scene_id": "scene_0001"}]}
EOF
cat > "${PROJ_AX}/shots/shot_0001/shot.toml" <<'EOF'
shot_id = "shot_0001"
start_time = 0.0
end_time = 2.0
duration = 2.0
anchor = "first_and_last"
seed = 42
characters = ["S1"]
audio_peaks = []
scene_id = "scene_0001"
EOF
cat > "${PROJ_AX}/story/scenes/scene_0001.json" <<'EOF'
{"scene_id": "scene_0001", "start_sec": 0.0, "end_sec": 2.0, "shot_count": 1,
 "location": "cafe", "time_of_day": "", "characters": [{"id": "S1", "appearance": "long hair", "wardrobe": ""}],
 "spatial_map": "S1 center", "eyeline_notes": "looks right",
 "dialogue_flow": "", "lighting": "",
 "axis_of_action": "S1 on screen-left looking right; S2 on screen-right looking left",
 "camera_grammar": "medium OTS with reverse-shot coverage"}
EOF
cat > "${PROJ_AX}/shots/shot_0001/prompt.ko.md" <<'EOF'
# 샷 연출 지시서: shot_0001 (0.00s ~ 2.00s)

## 1. 화면 및 카메라 연출
- 카메라 테스트.

## 4. 음향 및 공간음 (Soundscape)
- 조용한 실내음.

## 5. 배경음악 (BGM)
- 없음.
EOF
"${BIN}" sync-prompt --proj-dir "${PROJ_AX}" --force >/dev/null
grep -q "180도 축" "${PROJ_AX}/shots/shot_0001/prompt.ko.md" || { echo "FAIL: ko scene block missing 180도 축"; cat "${PROJ_AX}/shots/shot_0001/prompt.ko.md"; exit 1; }
grep -q "카메라 문법" "${PROJ_AX}/shots/shot_0001/prompt.ko.md" || { echo "FAIL: ko scene block missing 카메라 문법"; exit 1; }
grep -q "Axis of action" "${PROJ_AX}/shots/shot_0001/prompt.h3.txt" || { echo "FAIL: h3 scene block missing Axis of action"; cat "${PROJ_AX}/shots/shot_0001/prompt.h3.txt"; exit 1; }
grep -q "Camera grammar" "${PROJ_AX}/shots/shot_0001/prompt.h3.txt" || { echo "FAIL: h3 scene block missing Camera grammar"; exit 1; }
echo "PASS: [Check 3] axis + camera grammar serialized and injected."

echo ""
echo "==> All Issue 10 verifications passed."
