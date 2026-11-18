#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-11.sh
# Issue 11 Verification: original dialogue verbatim + 6-lang external subs +
# original audio default track + proof outputs.
#
# Criteria:
# 1. (A) EN/KO originals survive decompile verbatim; <d>[Lang] ...</d> tag
#    matches the body language; detect_dialogue_language unit tests pass.
# 2. (B) `assemble --smoke` writes 6 external .srt + .proof.srt with matching
#    cue counts and timecodes; cache reuse works.
# 3. (C) --proof build creates <movie>_smoke_proof.mp4.
# 4. (D) Final movie's Original Movie Audio track has default disposition.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue11-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 11 regression verifications in ${TMP_DIR}..."

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
# Check 1: original dialogue verbatim + language tag match
# ------------------------------------------------------------------------------
echo "--- [Check 1] original dialogue verbatim + <d> tag language match ---"
(cd "${REPO_ROOT}" && cargo test --bin rea-movie -- \
  test_detect_dialogue_language_script_priority \
  test_build_dialogue_keeps_original_and_matches_lang_tag \
  2>&1 | grep -q "2 passed")

# 1a. English original stays English (default ko dub_lang, no --translate-dub)
VID_EN="${TMP_DIR}/en.mp4"
make_video "${VID_EN}" "blue"
cat > "${TMP_DIR}/en.srt" <<'EOF'
1
00:00:00,500 --> 00:00:02,000
Hello world

2
00:00:02,500 --> 00:00:03,500
How are you today?

EOF
"${BIN}" decompile "${VID_EN}" -o "${TMP_DIR}/proj_en" --offline >/dev/null
KO_EN="${TMP_DIR}/proj_en/shots/shot_0001/prompt.ko.md"
H3_EN="${TMP_DIR}/proj_en/shots/shot_0001/prompt.h3.txt"
[[ -f "${KO_EN}" && -f "${H3_EN}" ]] || { echo "FAIL: EN prompts missing"; exit 1; }
grep -q "Hello world" "${KO_EN}" || { echo "FAIL: EN original lost in ko prompt"; cat "${KO_EN}"; exit 1; }
grep -q "<d>\[English\] Hello world" "${KO_EN}" || { echo "FAIL: EN tag mismatch in ko prompt"; cat "${KO_EN}"; exit 1; }
grep -q "<d>\[English\] Hello world" "${H3_EN}" || { echo "FAIL: EN tag mismatch in h3 prompt"; cat "${H3_EN}"; exit 1; }
if grep -q "<d>\[Korean\] Hello" "${KO_EN}"; then echo "FAIL: English mislabeled Korean"; exit 1; fi
echo "  [1a] English verbatim + [English] tag: PASS"

# 1b. Korean original stays Korean verbatim
VID_KO="${TMP_DIR}/ko.mp4"
make_video "${VID_KO}" "red"
cat > "${TMP_DIR}/ko.srt" <<'EOF'
1
00:00:00,500 --> 00:00:02,000
안녕하세요. 반갑습니다.

EOF
"${BIN}" decompile "${VID_KO}" -o "${TMP_DIR}/proj_ko" --offline >/dev/null
KO_KO="${TMP_DIR}/proj_ko/shots/shot_0001/prompt.ko.md"
H3_KO="${TMP_DIR}/proj_ko/shots/shot_0001/prompt.h3.txt"
grep -q "안녕하세요. 반갑습니다." "${KO_KO}" || { echo "FAIL: KO original lost"; cat "${KO_KO}"; exit 1; }
grep -q "<d>\[Korean\] 안녕하세요. 반갑습니다.</d>" "${KO_KO}" || { echo "FAIL: KO tag mismatch"; cat "${KO_KO}"; exit 1; }
grep -q "<d>\[Korean\] 안녕하세요. 반갑습니다.</d>" "${H3_KO}" || { echo "FAIL: KO tag mismatch h3"; cat "${H3_KO}"; exit 1; }
echo "  [1b] Korean verbatim + [Korean] tag: PASS"

# 1c. Silent shot stays silent in any dub_lang (issue 11 §3-2 d)
VID_SIL="${TMP_DIR}/sil.mp4"
make_video "${VID_SIL}" "green"
cat > "${TMP_DIR}/sil.srt" <<'EOF'
1
00:00:03,000 --> 00:00:03,800
Silence marker far away.

EOF
"${BIN}" decompile "${VID_SIL}" -o "${TMP_DIR}/proj_sil_en" --offline --dub-lang en --range 0-1 >/dev/null || true
# range 0-1 has no cue -> silent shot; check silent directive present
SIL_KO=$(find "${TMP_DIR}/proj_sil_en/shots" -name "prompt.ko.md" | head -1)
grep -q "대사 없음" "${SIL_KO}" || { echo "FAIL: silent shot missing 무음 지시문"; cat "${SIL_KO}"; exit 1; }
echo "  [1c] silent-shot directive consistent: PASS"
echo "PASS: [Check 1] originals verbatim, tags match."

# ------------------------------------------------------------------------------
# Check 2: six external subtitles + proof.srt
# ------------------------------------------------------------------------------
echo "--- [Check 2] six-language external subtitles + proof.srt ---"
PROJ="${TMP_DIR}/proj_sub"
mkdir -p "${PROJ}/shots/shot_0001/renders/smoke" "${PROJ}/shots/shot_0001/refs" "${PROJ}/story"
cat > "${PROJ}/timeline.json" <<'EOF'
{"shots": [{"shot_id": "shot_0001", "start_sec": 0.0, "end_sec": 4.0, "duration": 4.0,
  "is_chained_longtake": false, "anchor": "first_and_last", "assignee": "",
  "characters": ["S1"], "scene_id": ""}]}
EOF
ffmpeg -nostdin -v error -y -f lavfi -i "color=c=blue:s=320x240:d=4" \
  -f lavfi -i "sine=frequency=440:duration=4" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -ar 32000 -ac 2 -t 4 \
  "${PROJ}/shots/shot_0001/renders/smoke/shot_0001.mp4"
ffmpeg -nostdin -v error -y -f lavfi -i "color=c=red:s=320x240:d=4" \
  -f lavfi -i "sine=frequency=440:duration=4" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -ar 32000 -ac 2 -t 4 \
  "${PROJ}/shots/shot_0001/refs/original_clip.mp4"
cat > "${PROJ}/story/transcript.srt" <<'EOF'
1
00:00:00,500 --> 00:00:02,000
Hello world

2
00:00:02,500 --> 00:00:03,500
How are you today?

EOF

(cd "${TMP_DIR}" && MINIMAX_API_KEY="" "${BIN}" assemble --proj-dir "${PROJ}" --smoke --offline)
OUT_DIR="${PROJ}/outputs/smoke"
[[ -f "${OUT_DIR}/proj_sub_smoke.mp4" ]] || { echo "FAIL: final movie missing"; exit 1; }
[[ -f "${OUT_DIR}/proj_sub_compare.mp4" ]] || { echo "FAIL: compare movie missing"; exit 1; }
for lang in ko zh ja en fr de; do
  f="${OUT_DIR}/proj_sub_smoke.${lang}.srt"
  [[ -f "${f}" ]] || { echo "FAIL: missing ${f}"; exit 1; }
done
[[ -f "${OUT_DIR}/proj_sub_smoke.proof.srt" ]] || { echo "FAIL: proof.srt missing"; exit 1; }
# cue counts + timecode conformance
python3 - "${OUT_DIR}" <<'PY'
import re, sys
out = sys.argv[1]
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
src = parse(f"{out}/../../story/transcript.srt") if False else None
ref_times = [("00:00:00,500", "00:00:02,000"), ("00:00:02,500", "00:00:03,500")]
for lang in ["ko", "zh", "ja", "en", "fr", "de", "proof"]:
    suffix = "proof.srt" if lang == "proof" else f"{lang}.srt"
    cues = parse(f"{out}/proj_sub_smoke.{suffix}")
    assert len(cues) == 2, f"{lang}: cue count {len(cues)} != 2: {cues}"
    for (s, e, _), (rs, re_) in zip(cues, ref_times):
        assert s == rs and e == re_, f"{lang}: timecode mismatch {s}-->{e}"
    if lang == "proof":
        for (_, _, text) in cues:
            assert text.startswith("[S1] "), f"proof tag missing: {text}"
    print(f"  {lang}: 2 cues, timecodes OK")
PY
# cache files exist
for lang in ko zh ja en fr de; do
  [[ -f "${PROJ}/story/subtitles_cache/${lang}.srt" ]] || { echo "FAIL: cache ${lang}.srt missing"; exit 1; }
done
echo "PASS: [Check 2] 6-lang subs + proof.srt valid."

# ------------------------------------------------------------------------------
# Check 3: --proof video
# ------------------------------------------------------------------------------
echo "--- [Check 3] --proof video build ---"
(cd "${TMP_DIR}" && MINIMAX_API_KEY="" "${BIN}" assemble --proj-dir "${PROJ}" --smoke --offline --proof --force)
[[ -f "${OUT_DIR}/proj_sub_smoke_proof.mp4" ]] || { echo "FAIL: proof mp4 missing"; exit 1; }
DUR_PROOF=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "${OUT_DIR}/proj_sub_smoke_proof.mp4")
DUR_FINAL=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "${OUT_DIR}/proj_sub_smoke.mp4")
python3 -c "
pf = float('${DUR_PROOF}'); ff = float('${DUR_FINAL}')
assert abs(pf - ff) <= 0.5, f'proof {pf}s vs final {ff}s differ >0.5s'
"
echo "PASS: [Check 3] proof video created."

# ------------------------------------------------------------------------------
# Check 4: Original Movie Audio is the default track
# ------------------------------------------------------------------------------
echo "--- [Check 4] Original Movie Audio default disposition ---"
python3 - <<PY
import json, subprocess
p = "${OUT_DIR}/proj_sub_smoke.mp4"
out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a",
                      "-show_entries", "stream=index,codec_name:stream_disposition=default",
                      "-of", "json", p], capture_output=True, text=True)
assert out.returncode == 0, out.stderr
streams = json.loads(out.stdout).get("streams", [])
assert len(streams) >= 2, f"expected dual audio, got {streams}"
defaults = [s for s in streams if s.get("disposition", {}).get("default") == 1]
assert len(defaults) == 1, f"expected exactly one default audio, got {streams}"
# Original Movie Audio is mapped last (highest index); it must be the default.
assert defaults[0]["index"] == max(s["index"] for s in streams), \
    f"original (last) audio must be default: {streams}"
print(f"  audio streams: {streams} -> original (last) is default: PASS")
PY
echo "PASS: [Check 4] original audio is default."

echo ""
echo "==> All Issue 11 verifications passed."
