#!/usr/bin/env bash
# ==============================================================================
# brakingbad-s5e7.sh
# Breaking Bad S05E07 ("Say My Name") 앞부분 2분 추출 -> 한국어 대사 역공학 프로젝트 생성 -> 렌더링
# (한 줄씩 복사 & 붙여넣기(Copy & Paste)로 바로 실행할 수 있도록 모든 명령을 단일 행 절대경로로 작성)
# ==============================================================================

# [1단계] 공유 워크스페이스 디렉토리 준비
mkdir -p /rosenas/data/rea-movie-workspace/projects

# [2단계] 원본 MKV 앞부분 2분(00:00:00 ~ 00:02:00)을 잘라서 /rosenas/data/rea-movie-workspace/projects/ 에 복사 (H.264/AAC 호환 클립 생성)
ffmpeg -y -ss 00:00:00 -to 00:02:00 -i "/rosenas/data/video/KTV/Archive/A-Z/BreakingBad5/원본mkv/BreakingBad5.E7.1080p.mkv" -map 0:v:0 -map 0:a:0 -c:v libx264 -preset fast -crf 18 -c:a aac -ac 2 /rosenas/data/rea-movie-workspace/projects/BreakingBad5.E7.2min.mkv

# [3단계] 원본 영문 대사 자막(.en.srt)을 2분 클립과 동일한 이름(.srt)으로 복사 (샷별 대사 타임코드 자동 매핑용)
cp -f "/rosenas/data/video/KTV/Archive/A-Z/BreakingBad5/BreakingBad5.E7.1080p.en.srt" /rosenas/data/rea-movie-workspace/projects/BreakingBad5.E7.2min.srt

# [4단계] 2분 클립을 입력으로 하여 /rosenas/data/rea-movie-workspace/projects/breakingbad-s5e7/ 아래에 한국어 대사(--dub-lang ko) 역공학 프로젝트 생성
~/.local/bin/rea-movie /rosenas/data/rea-movie-workspace/projects/BreakingBad5.E7.2min.mkv -o /rosenas/data/rea-movie-workspace/projects/breakingbad-s5e7 --dub-lang ko --force

# [5단계] 생성된 샷별 한국어 대사(<d>[Korean] ...</d>) 확인
grep -H "<d>\[Korean\]" /rosenas/data/rea-movie-workspace/projects/breakingbad-s5e7/shots/*/prompt.ko.md

# [6단계 - 선택 A] 빠른 스토리/한국어 음성 확인용 Smoke 렌더링 및 좌우 1:1 비교 영상 조립 (608x352, 10 steps)
cd /rosenas/data/rea-movie-workspace/projects/breakingbad-s5e7 && ./run.sh --smoke

# [6단계 - 선택 B] 본편 고화질 Prod 렌더링 및 좌우 1:1 비교 영상 조립 (960x576, 50 steps)
# cd /rosenas/data/rea-movie-workspace/projects/breakingbad-s5e7 && ./run.sh
