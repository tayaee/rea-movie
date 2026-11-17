#!/usr/bin/env bash
# ==============================================================================
# breakingbad-s5e7-v3.sh
# Breaking Bad S05E07 ("Say My Name") 앞부분 2분 역공학 프로젝트 생성
# - 커스터마이징 레이어(customization/) 적용: Walter White 캐스팅 + KBO 야구모자 전역 트레이트
# - Git 업로드용 역공학 프로젝트 경로: $HOME/rea-movie-artifacts/h3-projects/breakingbad-s5e7-v3/
# - 입력 클립 및 렌더링/조립 출력 비디오 경로: $HOME/rea-movie-artifacts/h3-results/breakingbad-s5e7-v3/
# ==============================================================================

set -euo pipefail

PROJ_DIR="$HOME/rea-movie-artifacts/h3-projects/breakingbad-s5e7-v3"
WORKSPACE_DIR="$HOME/rea-movie-artifacts/h3-results/breakingbad-s5e7-v3"
SRC_VIDEO="/rosenas/data/video/KTV/Archive/A-Z/BreakingBad5/원본mkv/BreakingBad5.E7.1080p.mkv"
SRC_SRT="/rosenas/data/video/KTV/Archive/A-Z/BreakingBad5/BreakingBad5.E7.1080p.en.srt"

# [1단계] 프로젝트 및 워크스페이스 디렉토리 준비
mkdir -p "${PROJ_DIR}" "${WORKSPACE_DIR}"

# [2단계] 커스터마이징 레이어(customization/) 디렉토리 및 설정 파일 생성
mkdir -p "${PROJ_DIR}/customization/shots"

# 2-1. 인물 Bible: characters.toml
cat <<'EOF' > "${PROJ_DIR}/customization/characters.toml"
[characters.walter]
name_ko = "월터 화이트"
name_en = "Walter White"
appearance_en = "middle-aged man, bald head, goatee, hollow cheeks, dark sunken eyes"
appearance_ko = "중년 남성, 대머리, 콧수염, 함몰된 눈"
wardrobe_en = "black bomber jacket over a blue hoodie"
wardrobe_ko = "검정 볼트넥 재킷에 파란 후드"
face_ref = "assets/characters/walter/face.jpg"
voice_ref = "assets/characters/walter/voice.wav"
EOF

# 2-2. 전역 트레이트: traits.toml (Walter White에게만 KBO 야구모자 주입)
cat <<'EOF' > "${PROJ_DIR}/customization/traits.toml"
[[traits]]
id = "kbo_cap"
scope = "global"
target = "character:walter"
shots = []
EOF

# [3단계] 원본 MKV 앞부분 2분(00:00:00  00:02:00) 클립 추출 (파일이 있을 때)
if [[ -f "${SRC_VIDEO}" ]]; then
  ffmpeg -y -ss 00:00:00 -to 00:02:00 -i "${SRC_VIDEO}" \
    -map 0:v:0 -map 0:a:0 -c:v libx264 -preset fast -crf 18 -c:a aac -ac 2 \
    "${WORKSPACE_DIR}/breakingbad-s5e7.mkv"
fi

# [4단계] 원본 영문 자막 복사 (파일이 있을 때)
if [[ -f "${SRC_SRT}" ]]; then
  cp -f "${SRC_SRT}" "${WORKSPACE_DIR}/breakingbad-s5e7.srt"
fi

# [5단계] 역공학 decompile 실행
if [[ -f "${WORKSPACE_DIR}/breakingbad-s5e7.mkv" ]]; then
  rea-movie "${WORKSPACE_DIR}/breakingbad-s5e7.mkv" \
    -o "${PROJ_DIR}" \
    --dub-lang ko \
    --force
fi

# [6단계] 커스터마이징 블록 재적용 (멱등성 보장)
if [[ -d "${PROJ_DIR}/shots" ]]; then
  rea-movie customize apply --proj-dir "${PROJ_DIR}"
  rea-movie customize show --proj-dir "${PROJ_DIR}"
fi

# [7단계] 렌더링 안내
echo "==> Customization layer configured at ${PROJ_DIR}/customization"
echo "==> To render smoke profile: (cd ${PROJ_DIR} && ./run.sh --smoke)"
echo "==> To render prod profile:  (cd ${PROJ_DIR} && ./run.sh)"
