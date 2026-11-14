#!/usr/bin/env bash
# ==============================================================================
# breakingbad-s5e7.sh
# Breaking Bad S05E07 ("Say My Name") 앞부분 2분 역공학 프로젝트 생성 및 한국어 대사 + KBO 야구모자 렌더링
# - Git 업로드용 역공학 프로젝트 경로 (비디오/오디오 바이너리 없음): /rosenas/data/rea-movie-projects/breakingbad-s5e7/
# - 입력 클립 및 렌더링/조립 출력 비디오 경로 (../rea-movie-workspace): /rosenas/data/rea-movie-workspace/breakingbad-s5e7/
# - 한 줄씩 복사 & 붙여넣기(Copy & Paste)로 바로 실행할 수 있도록 모든 명령을 단일 행 절대경로로 작성
# ==============================================================================

# [1단계] 프로젝트 디렉토리(/rosenas/data/rea-movie-projects) 및 미디어 워크스페이스(/rosenas/data/rea-movie-workspace) 준비
mkdir -p /rosenas/data/rea-movie-projects /rosenas/data/rea-movie-workspace/breakingbad-s5e7

# [2단계] 원본 MKV 앞부분 2분(00:00:00 ~ 00:02:00)을 잘라서 미디어 워크스페이스(/rosenas/data/rea-movie-workspace/breakingbad-s5e7/)에 복사
ffmpeg -y -ss 00:00:00 -to 00:02:00 -i "/rosenas/data/video/KTV/Archive/A-Z/BreakingBad5/원본mkv/BreakingBad5.E7.1080p.mkv" -map 0:v:0 -map 0:a:0 -c:v libx264 -preset fast -crf 18 -c:a aac -ac 2 /rosenas/data/rea-movie-workspace/breakingbad-s5e7/breakingbad-s5e7.mkv

# [3단계] 원본 영문 대사 자막(.en.srt)을 2분 입력 클립과 동일한 이름(.srt)으로 워크스페이스에 복사 (샷별 대사 타임코드 자동 매핑용)
cp -f "/rosenas/data/video/KTV/Archive/A-Z/BreakingBad5/BreakingBad5.E7.1080p.en.srt" /rosenas/data/rea-movie-workspace/breakingbad-s5e7/breakingbad-s5e7.srt

# [4단계] 2분 클립을 입력으로 하여 /rosenas/data/rea-movie-projects/breakingbad-s5e7 에 Git 업로드용 역공학 프로젝트 생성 (한국어 대사 + 주인공 KBO 야구모자 착용)
~/.local/bin/rea-movie /rosenas/data/rea-movie-workspace/breakingbad-s5e7/breakingbad-s5e7.mkv -o /rosenas/data/rea-movie-projects/breakingbad-s5e7 --dub-lang ko --mod "주인공에게 KBO 야구모자를 씌워" --offline --force

# [5단계] 프로젝트 내 한국어 대사(<d>[Korean] ...</d>) 및 KBO 야구모자 지시 확인, 그리고 프로젝트 폴더 내 mkv/mp4/wav 부재 확인
grep -H -E "KBO|<d>\[Korean\]" /rosenas/data/rea-movie-projects/breakingbad-s5e7/shots/*/prompt.ko.md && find /rosenas/data/rea-movie-projects/breakingbad-s5e7 -type f \( -name "*.mp4" -o -name "*.mkv" -o -name "*.wav" \)

# [6단계 - 선택 A] 프로젝트 폴더에서 Smoke 렌더링 실행 (출력 비디오는 자동으로 ../rea-movie-workspace/breakingbad-s5e7/outputs/smoke/ 아래에 저장됨)
cd /rosenas/data/rea-movie-projects/breakingbad-s5e7 && ./run.sh --smoke

# [6단계 - 선택 B] 프로젝트 폴더에서 고화질 Prod 렌더링 실행 (출력 비디오는 자동으로 ../rea-movie-workspace/breakingbad-s5e7/outputs/prod/ 아래에 저장됨)
# cd /rosenas/data/rea-movie-projects/breakingbad-s5e7 && ./run.sh
