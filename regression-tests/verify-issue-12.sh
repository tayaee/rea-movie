#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-12.sh
# Issue 12 Verification: skill director/H3-expert playbook + README coverage,
# and one-line-request E2E (series-watch -> decompile -> mock-h3 -> proof assemble).
#
# Criteria:
# 1. (A) skills/rea-movie/SKILL.md + README.md document every Issue 8-11
#    expert keyword/section (§8 playbook included).
# 2. (B) One-line E2E on a synthetic multi-cut series video
#    (A-B-A dialogue + off-screen VO line + sub-4s shot):
#    series-watch -> decompile (sources/ cache, axis_of_action bible,
#    <Picture 1>/<Picture 2> prompts, off-screen no-lip-movement, verbatim) ->
#    render --smoke --mock-h3 -> assemble --smoke --proof (sub-4s retime,
#    6-lang .srt + .proof.srt + _proof.mp4 + original-audio default).
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SKILL="${REPO_ROOT}/skills/rea-movie/SKILL.md"
README="${REPO_ROOT}/README.md"
REVERSE_URL="${REPO_ROOT}/scripts/reverse-url.sh"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue12-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 12 regression verifications in ${TMP_DIR}..."

# ------------------------------------------------------------------------------
# Check 1: SKILL.md + README.md document all Issue 8-11 expert keywords
# ------------------------------------------------------------------------------
echo "--- [Check 1] skill/README expert documentation coverage ---"

# Each entry: FILE|FIXED-STRING (checked with grep -qF)
DOC_CHECKS=(
  # sources/ permanent cache (Issue 8)
  "SKILL|shots_cache"
  "SKILL|index.json"
  "SKILL|scene_cuts.json"
  "SKILL|probe.json"
  "SKILL|sources/yt-"
  # series-watch (Issue 8)
  "SKILL|series-watch"
  "SKILL|series_watch"
  "SKILL|world.json"
  "SKILL|characters.json"
  "SKILL|timeline-days.md"
  "README|series-watch"
  "README|series_watch"
  "README|world.json"
  # reverse-url.sh cache + episodes (Issue 8)
  "SKILL|--episodes"
  "README|--episodes"
  # min-shot-duration + sub-4s retime (Issue 9)
  "SKILL|--min-shot-duration"
  "SKILL|retime_render_clip"
  "README|--min-shot-duration"
  # M3 prompt pipeline (Issue 9)
  "SKILL|<Picture 1>"
  "SKILL|<Picture 2>"
  "SKILL|reference_line"
  "SKILL|Negative"
  "SKILL|<!-- rea-movie"
  "README|<Picture 1>"
  "README|<Picture 2>"
  # pro director grammar (Issue 10)
  "SKILL|A-B-A"
  "SKILL|axis_of_action"
  "SKILL|camera_grammar"
  "SKILL|delivery_mode"
  "SKILL|no lip movement"
  "README|axis_of_action"
  "README|delivery_mode"
  # verbatim dialogue + multilang subs + audio default (Issue 11)
  "SKILL|verbatim"
  "SKILL|detect_dialogue_language"
  "SKILL|--translate-dub"
  "SKILL|.proof.srt"
  "SKILL|default"
  "SKILL|--proof"
  "README|verbatim"
  "README|--translate-dub"
  "README|.proof.srt"
  "README|default"
  # §8 director & H3-expert playbook
  "SKILL|## 8."
  "SKILL|자율 프로토콜"
  "SKILL|Ablation"
  "SKILL|4점 세트"
  "SKILL|_compare.mp4"
)

for entry in "${DOC_CHECKS[@]}"; do
  file="${entry%%|*}"
  needle="${entry#*|}"
  target="${SKILL}"
  [[ "${file}" == "README" ]] && target="${README}"
  if ! grep -qF -- "${needle}" "${target}"; then
    echo "FAIL: ${file} missing required keyword: ${needle}"
    exit 1
  fi
done
echo "  SKILL.md + README.md keyword coverage: PASS (${#DOC_CHECKS[@]} checks)"

# reverse-url.sh pipeline wiring (static: no network in CI)
for needle in "sources/yt-" "series-watch" "--episodes" "Reuse cached"; do
  grep -qF -- "${needle}" "${REVERSE_URL}" || { echo "FAIL: reverse-url.sh missing: ${needle}"; exit 1; }
done
echo "  reverse-url.sh sources/ + series-watch + --episodes wiring: PASS"

# §8 playbook must spell out the 5-step protocol stages
for needle in "sources/" "series-watch" "axis_of_action" "리버스샷" "<Picture 1>" "Off-screen" "--mock-h3" "6개국어" "4점 세트"; do
  awk '/^## 8\./,/^## 9\./' "${SKILL}" | grep -qF -- "${needle}" || {
    echo "FAIL: §8 playbook missing stage keyword: ${needle}"; exit 1;
  }
done
echo "  §8 five-stage protocol stages: PASS"
echo "PASS: [Check 1] documentation coverage."

# ------------------------------------------------------------------------------
# Check 2: one-line-request E2E on a synthetic multi-cut series video
# ------------------------------------------------------------------------------
echo "--- [Check 2] one-line E2E: series-watch -> decompile -> mock-h3 -> proof ---"

ROOT="${TMP_DIR}/series"
mkdir -p "${ROOT}/sources" "${ROOT}/h3-projects"
SERIES_VID="${ROOT}/sources/series_ep01.mp4"

# A-B-A dialogue (red 2s / blue 2s / red 2s) + sub-4s short (green 1.5s) = 7.5s
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=red:s=320x240:d=2" \
  -f lavfi -i "color=c=blue:s=320x240:d=2" \
  -f lavfi -i "color=c=red:s=320x240:d=2" \
  -f lavfi -i "color=c=green:s=320x240:d=1.5" \
  -f lavfi -i "sine=frequency=440:duration=7.5" \
  -filter_complex "[0:v][1:v][2:v][3:v]concat=n=4:v=1:a=0[v]" \
  -map "[v]" -map "4:a" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 -t 7.5 \
  "${SERIES_VID}"

# Sibling subs: on-screen dialogue + off-screen (VO) line + short-shot button
cat > "${ROOT}/sources/series_ep01.ko.srt" <<'EOF'
1
00:00:00,300 --> 00:00:01,800
We have to go now.

2
00:00:02,300 --> 00:00:03,800
Stay with me in the blue room.

3
00:00:04,300 --> 00:00:05,800
(VO) 밖에 비가 내린다.

4
00:00:06,100 --> 00:00:07,200
Short green button.
EOF

# ---- ① series-watch: whole-series pre-analysis into <root>/story/ ----
echo "  [2.0] series-watch..."
"${BIN}" series-watch --out-root "${ROOT}" --offline "${SERIES_VID}" > "${TMP_DIR}/sw.log" 2>&1
for f in world.json characters.json timeline-days.md; do
  [[ -s "${ROOT}/story/${f}" ]] || { echo "FAIL: story/${f} missing"; cat "${TMP_DIR}/sw.log"; exit 1; }
done
echo "  [2.0] series-watch story/ created: PASS"

# ---- ② decompile: sources/ cache + bibles + prompts ----
echo "  [2.1] decompile..."
PROJ="${ROOT}/h3-projects/ep01"
# Media root rule (src/config.rs): <root>/h3-projects/<name> renders/outputs live in
# sibling <root>/h3-results/<name>/.
MEDIA="${ROOT}/h3-results/ep01"
"${BIN}" decompile "${SERIES_VID}" -o "${PROJ}" --offline > "${TMP_DIR}/dc.log" 2>&1
if grep -q "Warning: no series context" "${TMP_DIR}/dc.log"; then
  echo "FAIL: series context not picked up after series-watch"; exit 1;
fi
echo "  [2.1a] series context picked up (no warning): PASS"

SRC_CACHE_DIR=$(ls -d "${ROOT}"/sources/*/ | head -1)
for f in probe.json scene_cuts.json; do
  [[ -f "${SRC_CACHE_DIR}/${f}" ]] || { echo "FAIL: sources/ cache ${f} missing"; exit 1; }
done
[[ -d "${SRC_CACHE_DIR}/shots_cache" ]] || { echo "FAIL: shots_cache/ missing"; exit 1; }
echo "  [2.1b] sources/ cache (probe/scene_cuts/shots_cache): PASS"

mapfile -t SHOT_DIRS < <(ls -d "${PROJ}"/shots/shot_*/ | sort)
((${#SHOT_DIRS[@]} >= 3)) || { echo "FAIL: expected >=3 shots, got ${#SHOT_DIRS[@]}"; exit 1; }
echo "  [2.1c] multi-cut split into ${#SHOT_DIRS[@]} shots: PASS"

SCENE_JSON=$(ls "${PROJ}"/story/scenes/scene_*.json 2>/dev/null | head -1)
[[ -n "${SCENE_JSON}" ]] || { echo "FAIL: scene bible missing"; exit 1; }
grep -q "axis_of_action" "${SCENE_JSON}" || { echo "FAIL: axis_of_action missing in scene bible"; exit 1; }
echo "  [2.1d] scene bible axis_of_action: PASS"

# Off-screen (VO) shot: KO 화면 밖 + 입을 다문, H3 no lip movement + off-screen, no synchronized lip sync
VO_HIT=""
for d in "${SHOT_DIRS[@]}"; do
  if grep -q "비가 내린다" "${d}/prompt.ko.md" 2>/dev/null; then VO_HIT="${d}"; break; fi
done
[[ -n "${VO_HIT}" ]] || { echo "FAIL: VO cue not found in any shot prompt"; exit 1; }
grep -q "화면 밖" "${VO_HIT}/prompt.ko.md" || { echo "FAIL: VO ko missing 화면 밖"; exit 1; }
grep -q "no lip movement" "${VO_HIT}/prompt.h3.txt" || { echo "FAIL: VO h3 missing no lip movement"; exit 1; }
grep -q "off-screen" "${VO_HIT}/prompt.h3.txt" || { echo "FAIL: VO h3 missing off-screen"; exit 1; }
if grep -q "synchronized lip movement" "${VO_HIT}/prompt.h3.txt"; then
  echo "FAIL: VO h3 must NOT lip-sync"; exit 1;
fi
echo "  [2.1e] off-screen VO (no lip movement): PASS"

# Verbatim originals + <d>[Language]> match + <Picture 1>/<Picture 2> delta
grep -q "We have to go now" "${PROJ}/shots/shot_0001/prompt.ko.md" || { echo "FAIL: EN verbatim lost"; exit 1; }
grep -q "<d>\[English\] We have to go now" "${PROJ}/shots/shot_0001/prompt.h3.txt" || { echo "FAIL: EN tag mismatch"; exit 1; }
grep -q "<d>\[Korean\]" "${VO_HIT}/prompt.h3.txt" || { echo "FAIL: KO tag mismatch on VO line"; exit 1; }
grep -q "<Picture 1>" "${PROJ}/shots/shot_0001/prompt.h3.txt" || { echo "FAIL: <Picture 1> missing"; exit 1; }
grep -q "<Picture 2>" "${PROJ}/shots/shot_0001/prompt.h3.txt" || { echo "FAIL: <Picture 2> missing"; exit 1; }
if grep -q "<!-- rea-movie" "${PROJ}/shots/shot_0001/prompt.h3.txt"; then
  echo "note: disk prompt.h3.txt keeps idempotence markers (expected)"
fi
echo "  [2.1f] verbatim + language tags + Picture delta: PASS"

# ---- ③ render --smoke --mock-h3 ----
echo "  [2.2] render --smoke --mock-h3..."
(cd "${TMP_DIR}" && MINIMAX_API_KEY="" "${BIN}" render --proj-dir "${PROJ}" --smoke --mock-h3 >/dev/null)
for d in "${SHOT_DIRS[@]}"; do
  sid=$(basename "${d}")
  [[ -s "${MEDIA}/shots/${sid}/renders/smoke/${sid}.mp4" ]] || { echo "FAIL: mock render missing for ${sid}"; exit 1; }
done
echo "  [2.2] mock-h3 renders for all shots: PASS"

# ---- sub-4s retime proof: replace one short render with a 4s red/blue H3-style clip ----
# A plain `-t` trim would keep only red; retime must preserve the blue tail.
TAMPER_SHOT=""
TAMPER_TARGET=""
for d in "${SHOT_DIRS[@]}"; do
  sid=$(basename "${d}")
  dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "${MEDIA}/shots/${sid}/renders/smoke/${sid}.mp4")
  if python3 -c "import sys; sys.exit(0 if float('${dur}') < 3.9 else 1)"; then
    TAMPER_SHOT="${sid}"; TAMPER_TARGET="${dur}"; break
  fi
done
[[ -n "${TAMPER_SHOT}" ]] || { echo "FAIL: no sub-4s shot render to tamper"; exit 1; }
ffmpeg -nostdin -v error -y \
  -f lavfi -i "color=c=red:s=608x352:d=2" \
  -f lavfi -i "color=c=blue:s=608x352:d=2" \
  -f lavfi -i "sine=frequency=440:duration=4" \
  -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" \
  -map "[v]" -map "2:a" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -ar 32000 -ac 2 -t 4 \
  "${MEDIA}/shots/${TAMPER_SHOT}/renders/smoke/${TAMPER_SHOT}.mp4"

mean_rgb() {
  local video="$1" at="$2"
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

# ---- ④ assemble --smoke --proof ----
echo "  [2.3] assemble --smoke --proof..."
(cd "${TMP_DIR}" && MINIMAX_API_KEY="" "${BIN}" assemble --proj-dir "${PROJ}" --smoke --offline --proof --force)
OUT_DIR="${MEDIA}/outputs/smoke"
MOVIE_BASENAME=$(basename "${SERIES_VID}" .mp4)
FINAL="${OUT_DIR}/${MOVIE_BASENAME}_smoke.mp4"
COMPARE="${OUT_DIR}/${MOVIE_BASENAME}_compare.mp4"
PROOF_MP4="${OUT_DIR}/${MOVIE_BASENAME}_smoke_proof.mp4"
[[ -f "${FINAL}" && -f "${COMPARE}" ]] || { echo "FAIL: final/compare movie missing"; ls "${OUT_DIR}"; exit 1; }
for lang in ko zh ja en fr de; do
  [[ -s "${OUT_DIR}/${MOVIE_BASENAME}_smoke.${lang}.srt" ]] || { echo "FAIL: missing .${lang}.srt"; exit 1; }
done
[[ -s "${OUT_DIR}/${MOVIE_BASENAME}_smoke.proof.srt" ]] || { echo "FAIL: .proof.srt missing"; exit 1; }
[[ -f "${PROOF_MP4}" ]] || { echo "FAIL: _proof.mp4 missing"; exit 1; }
echo "  [2.3a] 6-lang .srt + .proof.srt + _proof.mp4: PASS"

python3 - "${OUT_DIR}/${MOVIE_BASENAME}_smoke" <<'PY'
import re, sys
base = sys.argv[1]
def parse(p):
    txt = open(p, encoding="utf-8").read()
    blocks = [b for b in txt.replace("\r\n", "\n").split("\n\n") if b.strip()]
    cues = []
    for b in blocks:
        m = re.search(r"(\d{2}:\d{2}:\d{2}[,.]\d{3})\s*-->\s*(\d{2}:\d{2}:\d{2}[,.]\d{3})", b)
        assert m, f"bad SRT block in {p}: {b[:80]}"
        lines = [l for l in b.splitlines() if l.strip()]
        cues.append((m.group(1), m.group(2), " ".join(lines[2:])))
    return cues
ref_times = [("00:00:00,300", "00:00:01,800"), ("00:00:02,300", "00:00:03,800"),
             ("00:00:04,300", "00:00:05,800"), ("00:00:06,100", "00:00:07,200")]
for lang in ["ko", "zh", "ja", "en", "fr", "de", "proof"]:
    suffix = "proof.srt" if lang == "proof" else f"{lang}.srt"
    cues = parse(f"{base}.{suffix}")
    assert len(cues) == 4, f"{lang}: cue count {len(cues)} != 4"
    for (s, e, _), (rs, re_) in zip(cues, ref_times):
        assert s == rs and e == re_, f"{lang}: timecode {s}-->{e} != {rs}-->{re_}"
print("  [2.3b] subtitle cue counts + timecodes match transcript: PASS")
PY

# Retimed tail check: tampered shot's assembled segment must end blue (last_frame preserved)
read -r TAMPER_START TAMPER_END <<< "$(python3 -c "
import json
tl = json.load(open('${PROJ}/timeline.json'))
for s in tl['shots']:
    if s['shot_id'] == '${TAMPER_SHOT}':
        print(f\"{s['start_sec']} {s['end_sec']}\")
")"
TAIL_AT=$(python3 -c "print(float('${TAMPER_END}') - 0.2)")
HEAD_AT=$(python3 -c "print(float('${TAMPER_START}') + 0.2)")
read -r HR HG HB <<< "$(mean_rgb "${FINAL}" "${HEAD_AT}")"
read -r TR TG TB <<< "$(mean_rgb "${FINAL}" "${TAIL_AT}")"
python3 -c "
hr, hb, tr, tb = float('${HR}'), float('${HB}'), float('${TR}'), float('${TB}')
assert hr > 120 and hr > hb, f'tampered head should stay red, got R={hr} B={hb}'
assert tb > 80 and tb > tr, f'retimed tail must preserve blue last_frame, got R={tr} B={tb}'
print(f'  [2.3c] sub-4s retime keeps last_frame (head R={hr} tail B={tb}): PASS')
"

# Final duration ≈ source duration; proof mp4 ≈ final
python3 -c "
import subprocess
def dur(p):
    return float(subprocess.run(['ffprobe','-v','error','-show_entries','format=duration','-of','csv=p=0',p],
                capture_output=True, text=True).stdout.strip())
src, fin, prf = dur('${SERIES_VID}'), dur('${FINAL}'), dur('${PROOF_MP4}')
assert abs(fin - src) <= 0.6, f'final {fin}s vs source {src}s differ >0.6s'
assert abs(prf - fin) <= 0.5, f'proof {prf}s vs final {fin}s differ >0.5s'
print(f'  [2.3d] durations source={src:.2f}s final={fin:.2f}s proof={prf:.2f}s: PASS')
"

# Original Movie Audio must be the default track
python3 - <<PY
import json, subprocess
p = "${FINAL}"
out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a",
                      "-show_entries", "stream=index,codec_name:stream_disposition=default",
                      "-of", "json", p], capture_output=True, text=True)
assert out.returncode == 0, out.stderr
streams = json.loads(out.stdout).get("streams", [])
assert len(streams) >= 2, f"expected dual audio, got {streams}"
defaults = [s for s in streams if s.get("disposition", {}).get("default") == 1]
assert len(defaults) == 1, f"expected exactly one default audio, got {streams}"
assert defaults[0]["index"] == max(s["index"] for s in streams), \
    f"original (last) audio must be default: {streams}"
print("  [2.3e] original audio is default track: PASS")
PY
echo "PASS: [Check 2] one-line E2E (watch -> decompile -> mock-h3 -> proof)."

echo ""
echo "==> All Issue 12 verifications passed."
