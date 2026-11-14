#!/usr/bin/env bash
# download_samples.sh
# 퍼블릭 도메인(저작권 만료) 고전 명작 영화 5편을 sample_movies/ 디렉토리에 다운로드합니다.
# 이미 다운로드된 영상 파일이 존재하면 기본적으로 건너뛰며(Skip), --force 지정 시 재다운로드합니다.
#
# 사용법:
#   ./download_samples.sh                # 5편 전체 다운로드 (존재 시 스킵)
#   ./download_samples.sh 2              # 2번(슈퍼맨 1941, 10분 단편)만 다운로드
#   ./download_samples.sh 1 --force      # 1번(샤레이드 1963) 강제 재다운로드
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SAMPLE_DIR="$REPO_ROOT/sample_movies"
mkdir -p "$SAMPLE_DIR"

FORCE=0
FILTER_ID=""

for arg in "$@"; do
    case "$arg" in
        --force|-f)
            FORCE=1
            ;;
        [1-5]|0[1-5])
            FILTER_ID="$(printf "%02d" "$((10#$arg))")"
            ;;
        -h|--help)
            echo "Usage: $0 [1..5] [--force]"
            exit 0
            ;;
    esac
done

if ! command -v yt-dlp >/dev/null 2>&1; then
    echo "Error: yt-dlp 가 설치되어 있지 않습니다." >&2
    exit 1
fi

# ID | 파일 베이스명 | YouTube URL | 제목
MOVIES=(
    "01|01_charade_1963|https://www.youtube.com/watch?v=6ROvPFaUl80|Charade (1963, Public Domain)"
    "02|02_superman_mad_scientist_1941|https://www.youtube.com/watch?v=9UJphNPwDfk|Superman: The Mad Scientist (1941, Public Domain)"
    "03|03_night_of_the_living_dead_1968|https://www.youtube.com/watch?v=SEcsfcDCoVg|Night of the Living Dead (1968, Public Domain)"
    "04|04_his_girl_friday_1940|https://www.youtube.com/watch?v=MWVZXblHWXI|His Girl Friday (1940, Public Domain)"
    "05|05_sherlock_jr_1924|https://www.youtube.com/watch?v=gN5OLNaFk0I|Sherlock Jr. (1924, Public Domain)"
)

for entry in "${MOVIES[@]}"; do
    IFS='|' read -r id base url title <<< "$entry"
    if [ -n "$FILTER_ID" ] && [ "$id" != "$FILTER_ID" ]; then
        continue
    fi

    target_mp4="$SAMPLE_DIR/${base}.mp4"
    target_md="$SAMPLE_DIR/${base}.md"

    echo "=================================================================="
    echo " 🎬 [$id/05] $title"
    echo "    Video : $target_mp4"
    echo "    Doc   : $target_md"
    echo "=================================================================="

    if [ ! -f "$target_md" ]; then
        echo "WARN: $target_md 설명 파일이 없습니다."
    fi

    if [ -f "$target_mp4" ] && [ -s "$target_mp4" ] && [ "$FORCE" -eq 0 ]; then
        echo "✅ Skip: 이미 존재합니다 ($(du -h "$target_mp4" | cut -f1)). 재다운로드하려면 --force 옵션을 주세요."
        continue
    fi

    echo "⬇️  Downloading from $url ..."
    yt-dlp \
        --no-playlist \
        --js-runtimes node \
        --remote-components ejs:github \
        --ignore-errors \
        -f "bv*[height<=720][ext=mp4]+ba[ext=m4a]/b[height<=720][ext=mp4]/b[ext=mp4]/bv*+ba/b" \
        --merge-output-format mp4 \
        --write-subs --sub-langs "en" --embed-subs \
        -o "$target_mp4" \
        "$url"

    ls -lh "$target_mp4"
done

echo ""
echo "🎉 모든 샘플 영화 준비 완료: $SAMPLE_DIR"
