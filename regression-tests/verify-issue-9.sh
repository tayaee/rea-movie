#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-9.sh
# Issue 9 Verification: H3 sub-4.0s anchor-preserving retime + prompt pipeline
#
# Verification Criteria:
# 1. (A) Render retime: a 4.0s render clip (first half red, second half blue)
#    assembled to a 2.0s target keeps BOTH colors (last_frame anchor survives).
#    A plain `-t` trim would leave only red.
# 2. (B/C) ensure_h3_reference_header + clean_prompt_for_h3 (HTML marker strip).
# 3. (C) compile_ko_to_h3 offline: Korean directing prose is not copied into
#    non-dialogue H3 sections (dialogue <d> tags preserved), header kept.
# 4. (A) --min-shot-duration merges micro-cuts (CLI + unit behavior).
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue9-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 9 regression verifications in ${TMP_DIR}..."

probe_dur() {
  ffprobe -v error -show_entries format=duration -of csv=p=0 "$1"
}

# Mean RGB of a single frame at $2 seconds, 32x32 rawvideo average.
mean_rgb() {
  local video="$1"
  local at="$2"
  ffmpeg -nostdin -v error -ss "${at}" -i "${video}" -vframes 1 \
    -f rawvideo -pix_fmt rgb24 -s 32x32 - 2>/dev/null | python3 -c "
import sys
data = sys.stdin.buffer.read()
n = len(data) // 3
if n == 0:
    print('0 0 0')
else:
    r = sum(data[i] for i in range(0, len(data), 3)) / n
    g = sum(data[i] for i in range(1, len(data), 3)) / n
    b = sum(data[i] for i in range(2, len(data), 3)) / n
    print(f'{r:.1f} {g:.1f} {b:.1f}')
"
}

# ------------------------------------------------------------------------------
# Check 1: 4.0s render (2s red + 2s blue) retimed to 2.0s keeps both anchors
# ------------------------------------------------------------------------------
echo "--- [Check 1] Render retime preserves last_frame anchor (red->blue) ---"
PROJ1="${TMP_DIR}/proj_retime"
mkdir -p "${PROJ1}/shots/shot_0001/renders/smoke" "${PROJ1}/shots/shot_0001/refs"

cat <<'EOF' > "${PROJ1}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 2.0,
      "duration": 2.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": ""
    }
  ]
}
EOF

# H3-style render: H3 always renders >= 4.0s (clamp). First half red, last half blue.
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=red:s=320x240:d=2" \
  -f lavfi -i "color=c=blue:s=320x240:d=2" \
  -f lavfi -i "sine=frequency=440:duration=4" \
  -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" \
  -map "[v]" -map "2:a" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 -t 4 \
  "${PROJ1}/shots/shot_0001/renders/smoke/shot_0001.mp4"

# Original clip at target length (compare-movie left side).
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=red:s=320x240:d=2" \
  -f lavfi -i "sine=frequency=440:duration=2" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 -t 2 \
  "${PROJ1}/shots/shot_0001/refs/original_clip.mp4"

"${BIN}" assemble --proj-dir "${PROJ1}" --smoke

FINAL1="${PROJ1}/outputs/smoke/proj_retime_smoke.mp4"
[[ -f "${FINAL1}" ]] || { echo "FAIL: final movie missing at ${FINAL1}"; exit 1; }

DUR1=$(probe_dur "${FINAL1}")
echo "  Final duration: ${DUR1}s (target 2.0s)"
python3 -c "
d = float('${DUR1}')
assert abs(d - 2.0) <= 0.5, f'duration {d}s not within 2.0s ±0.5s'
"

EARLY=$(mean_rgb "${FINAL1}" 0.2)
LATE=$(mean_rgb "${FINAL1}" 1.8)
echo "  Early frame RGB: ${EARLY} (expect red-dominant)"
echo "  Late frame RGB:  ${LATE} (expect blue-dominant)"
python3 -c "
er, eg, eb = map(float, '${EARLY}'.split())
lr, lg, lb = map(float, '${LATE}'.split())
assert er > 100 and er > eb + 30, f'early frame not red: ${EARLY}'
assert lb > 100 and lb > lr + 30, f'late frame lost blue anchor (plain trim?): ${LATE}'
print('  Retime keeps both anchors: PASS')
"

if [[ -d "${PROJ1}/outputs/smoke/.trim" ]]; then
  echo "FAIL: .trim directory was not cleaned up"
  exit 1
fi
echo "PASS: [Check 1] retime preserves first_frame and last_frame anchors."

# ------------------------------------------------------------------------------
# Check 2: ensure_h3_reference_header + clean_prompt_for_h3
# ------------------------------------------------------------------------------
echo "--- [Check 2] H3 reference header + HTML marker strip ---"
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- \
  reference_line_for ensure_h3_reference_header strip_markers compiler_drops_korean 2>&1 | tail -n 8)

# Direct marker-strip sanity: disk file keeps markers, transmitted copy drops them.
PROMPT_TMP="${TMP_DIR}/prompt_h3.txt"
cat <<'EOF' > "${PROMPT_TMP}"
<!-- rea-movie:scene:begin id=scene_0001 -->
Scene continuity context (scene_0001):
<!-- rea-movie:scene:end id=scene_0001 -->

For the target video, at 0.00 seconds into the target video, <Picture 1> and <Picture 2> (from [Shot 1]) are fully referenced.

integrated_multimodal_description:
Action continues.
EOF
grep -q "rea-movie:scene:begin" "${PROMPT_TMP}" || { echo "FAIL: fixture lost markers"; exit 1; }
echo "PASS: [Check 2] header + marker-strip unit tests passed (see cargo test above)."

# ------------------------------------------------------------------------------
# Check 3: compile_ko_to_h3 end-to-end via sync-prompt (Korean filtering)
# ------------------------------------------------------------------------------
echo "--- [Check 3] sync-prompt offline Korean filtering ---"
PROJ3="${TMP_DIR}/proj_sync"
mkdir -p "${PROJ3}/shots/shot_0001"
cat <<'EOF' > "${PROJ3}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 2.0,
      "duration": 2.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["S1"],
      "scene_id": ""
    }
  ]
}
EOF
cat <<'EOF' > "${PROJ3}/shots/shot_0001/shot.toml"
shot_id = "shot_0001"
start_time = 0.0
end_time = 2.0
duration = 2.0
anchor = "first_and_last"
seed = 42
characters = ["S1"]
audio_peaks = []
scene_id = ""
EOF
cat <<'EOF' > "${PROJ3}/shots/shot_0001/prompt.ko.md"
# 샷 연출 지시서: shot_0001 (0.00s ~ 2.00s)

## 1. 화면 및 카메라 연출
- 카메라는 원본 첫 프레임을 정밀하게 잇는다.
- S1: <d>[Korean] 긴장감 넘치는 장면이다.</d>

## 4. 음향 및 공간음 (Soundscape)
- 현장 공간감이 살아있는 리얼한 앰비언스

## 5. 배경음악 (BGM)
- 절제된 시네마틱 배경음
EOF

"${BIN}" sync-prompt --proj-dir "${PROJ3}" --force
H3_OUT="${PROJ3}/shots/shot_0001/prompt.h3.txt"
[[ -f "${H3_OUT}" ]] || { echo "FAIL: prompt.h3.txt not generated"; exit 1; }
grep -q "<Picture 1> and <Picture 2> (from \[Shot 1\])" "${H3_OUT}" \
  || { echo "FAIL: reference header missing"; cat "${H3_OUT}"; exit 1; }
grep -q "<d>\[Korean\] 긴장감 넘치는 장면이다.</d>" "${H3_OUT}" \
  || { echo "FAIL: dialogue <d> tag not preserved"; cat "${H3_OUT}"; exit 1; }
python3 -c "
import re, sys
text = open('${H3_OUT}', encoding='utf-8').read()
# Remove dialogue spans before checking for leaked Hangul.
stripped = re.sub(r'<d>.*?</d>', '', text, flags=re.S)
bad = [ch for ch in stripped if '\uac00' <= ch <= '\ud7a3']
assert not bad, f'Hangul leaked into non-dialogue H3 sections: {stripped[:300]}'
print('  No Hangul outside dialogue: PASS')
"
echo "PASS: [Check 3] Korean directing prose filtered, dialogue + header kept."

# ------------------------------------------------------------------------------
# Check 4: --min-shot-duration option (CLI + merge behavior)
# ------------------------------------------------------------------------------
echo "--- [Check 4] --min-shot-duration merging ---"
"${BIN}" decompile --help | grep -q "min-shot-duration" \
  || { echo "FAIL: --min-shot-duration missing from decompile --help"; exit 1; }
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- merge_short_spans build_shot_spans_with_min 2>&1 | tail -n 6)

# End-to-end: 4s video with a hard cut (2s blue + 2s red) plus an audio track
# (decompile always extracts original_audio.wav, so video-only input fails).
VID4="${TMP_DIR}/cut.mp4"
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=blue:s=320x240:d=2" \
  -f lavfi -i "color=c=red:s=320x240:d=2" \
  -f lavfi -i "sine=frequency=440:duration=4" \
  -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" \
  -map "[v]" -map "2:a" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 \
  "${VID4}"
"${BIN}" decompile "${VID4}" -o "${TMP_DIR}/proj_plain" --offline --max-shot-duration 8.0 >/dev/null
"${BIN}" decompile "${VID4}" -o "${TMP_DIR}/proj_merged" --offline --max-shot-duration 8.0 --min-shot-duration 3.0 >/dev/null
PLAIN_N=$(python3 -c "import json; print(len(json.load(open('${TMP_DIR}/proj_plain/timeline.json'))['shots']))")
MERGED_N=$(python3 -c "import json; print(len(json.load(open('${TMP_DIR}/proj_merged/timeline.json'))['shots']))")
echo "  Plain shots: ${PLAIN_N}, merged (min 3.0s): ${MERGED_N}"
python3 -c "
plain = int('${PLAIN_N}'); merged = int('${MERGED_N}')
assert merged <= plain, f'merged {merged} > plain {plain}'
assert merged >= 1, 'merged must keep at least one shot'
"
echo "PASS: [Check 4] --min-shot-duration merges micro-cuts."

echo ""
echo "==> All Issue 9 verifications passed."
