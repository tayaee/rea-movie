# Issue 1: Rust 코어 CLI 및 영화 역공학(`decompile`) 파이프라인 구현

## 1. 배경 및 목표
`rea-movie` (`reverse-engineer-any-movie`)의 핵심 기반인 Rust Cargo 프로젝트를 구성하고, 입력 영상(`mkv`/`mp4`)을 분석하여 샷 분할, `+2/-2` 프레임 앵커 추출, 타임스탬프 번인 스토리보드(`contact_sheet.jpg`), 오디오 피크 타임코드, 등장인물/보이스 지문 도감, 한국어 연출 지시서(`prompt.ko.md`) 및 MiniMax-H3 공식 스키마 영문 프롬프트(`prompt.h3.txt`), 그리고 단일 진입점 스크립트(`run.sh`)를 생성하는 역공학 파이프라인을 구현한다.

## 2. 상세 요구사항

1. **Cargo 프로젝트 및 CLI 인터페이스 (`rea-movie`)**:
   - 바이너리 이름: `rea-movie`
   - 기본 실행(서브커맨드 생략): `rea-movie <input.mkv|mp4> [--range START-END] [-o <output_dir>] [--max-shot-duration 8.0] [--force] [--run] [--smoke]`
     - 영상 파일이 첫 번째 인자로 주어지면 자동으로 역공학(`decompile`)을 수행하여 `./<파일명_스탬프>/` (또는 `-o` 지정 경로)에 프로젝트를 생성한다.
   - 명시적 서브커맨드 지원:
     - `rea-movie decompile <input> [OPTS]`
     - `rea-movie render [OPTS]` (`--shot <ID_OR_RANGE>`, `--range <TIME_RANGE>`, `--smoke`, `--force`, `--force-video-only`)
     - `rea-movie sync-prompt [--shot <ID>] [--force]`
     - `rea-movie assemble [--smoke]`
     - `rea-movie pool status`
     - `rea-movie mcp`

2. **프로젝트 디렉토리 레이아웃 및 설정 파일 생성**:
   - `rea-movie.toml`:
     - DGX Spark 노드 풀 기본값: `nodes = ["spark1.local", "spark2.local"]`, `base_port = 8000`, `remote_workspace = "~/rea-movie-workspace"`
     - 샷 분할 기본값: `max_shot_duration = 8.0` (8초 초과 시 OOM 방지를 위해 자동 분할)
     - 프로필 설정:
       - `[profiles.prod]`: `width = 960`, `height = 576`, `fps = 24`, `steps = 50`, `flow_shift = 12.0`, `audio_flow_shift = 3.0`
       - `[profiles.smoke]`: `width = 608`, `height = 352`, `fps = 24`, `steps = 10`, `flow_shift = 12.0`, `audio_flow_shift = 3.0`
     - MiniMax-M3 설정: `api_url = "https://api.minimax.io/v1/chat/completions"`, `model = "MiniMax-M3"`, `api_key_env = "MINIMAX_API_KEY"` (절대 파일에 실제 API 키를 저장하지 말 것)
   - `timeline.json`: 전체 샷 목록(`shot_id`, `start_sec`, `end_sec`, `duration`, `is_chained_longtake`, `anchor`, `assignee`)
   - `story/transcript.srt`, `story/screenplay.md`, `story/characters.toml` (화자 `S1`, `S2` 및 보이스 지문 `voice_fingerprint` 포함)
   - `assets/characters/S1_.../face.jpg`, `voice.wav`

3. **단일 카메라 컷 엄수 샷 분할 및 레퍼런스 추출 (`ffmpeg` 연동)**:
   - `ffmpeg` `scenedetect` 필터(`select='gt(scene,0.25)',showinfo`)로 실제 카메라 하드 컷 경계를 검출한다.
   - 두 개의 카메라 컷을 하나의 `fl2va` 샷으로 묶지 않으며, 단일 컷이 `max_shot_duration`(기본 `8.0`초)을 초과할 때만 `8.0`초 이하 서브샷으로 분할한다.
   - 각 샷(`shots/shot_XXXX/`, 4자리 0-패딩 번호 예: `shot_0001`, `shot_0005`)마다 다음을 생성한다:
     - `shot.toml`: `shot_id`, `start_time`, `end_time`, `duration`, `anchor = "first_and_last"` (기본값, `"first_only"`, `"ref_only"`, `"none"` 지원), `seed`, `characters`, `audio_peaks`
     - `refs/original_clip.mp4`: 해당 샷 구간의 원본 비디오+오디오 클립
     - `refs/original_audio.wav`: 해당 샷 구간의 오디오 파일 및 피크 타임코드 분석
     - `refs/first_frame.jpg`: 블렌딩 잔상을 피하기 위해 컷 시작 `+2프레임`(또는 `min(duration*0.1, 0.08)`초)에서 추출
     - `refs/last_frame.jpg`: 컷 끝 `-2프레임`(또는 `max(0, duration - 0.08)`초)에서 추출
     - `refs/contact_sheet.jpg`: 샷 내부를 1초 간격으로 샘플링하고 각 타일 좌상단에 `[0.0s]`, `[1.0s]` 타임스탬프를 표시해 가로/그리드로 합친 스토리보드 이미지
     - `renders/prod/` 및 `renders/smoke/` 빈 디렉토리 사전 생성

4. **2계층 프롬프트 생성 (`prompt.ko.md` & `prompt.h3.txt`) 및 멱등성(Skip-by-default)**:
   - 환경변수 `MINIMAX_API_KEY`가 설정되어 있으면 `MiniMax-M3` 비전 API를 호출하고, 미설정 또는 `--offline` 시 결정론적 분석 템플릿으로 생성한다.
   - `prompt.ko.md`: 사람이 편집하기 쉬운 한국어 연출 지시서 (화면/카메라 연출, 초 단위 동작 궤적, 대사, 공간음/피크 효과음, 배경음악 섹션 포함).
   - `prompt.h3.txt`: MiniMax-H3 공식 프롬프트 포맷 준수:
     - 첫 줄 정렬 지시문: `For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot 1]) is fully referenced.`
     - `integrated_multimodal_description:` 초 단위 동작 궤적(`From 0.0s to ...`), 화자 보이스 지문 + `<d>[Language] 대사</d>`, 피크 효과음 타이밍 포함
     - `overall_soundscape:`
     - `non_diegetic_music:`
   - 타겟 파일(`prompt.ko.md`, `prompt.h3.txt`, `refs/*`)이 이미 존재하면 `--force`가 없는 한 덮어쓰지 않고 건너뛴다(Skip-by-default).

5. **단일 진입점 `./run.sh` 생성**:
   - 생성된 프로젝트 루트에 실행 권한(`chmod +x`)이 부여된 `run.sh`를 생성한다.
   - `./run.sh` (기본 Prod 렌더링 + 조립), `./run.sh --smoke` (Smoke 저해상도 렌더링 + 조립), `./run.sh 0005 --force` (특정 샷 재생성 + 조립), `./run.sh 0005 --force-video-only`, `./run.sh 0001-0005 --force` 인자를 모두 처리하여 `rea-movie` 바이너리를 호출하도록 작성한다.

## 3. 검수 기준
- `cargo test` 및 `cargo build --release`가 에러 없이 성공해야 한다.
- `./regression-tests/verify-issue-1.sh` 실행 시 합성된 테스트 영상으로부터 프로젝트 역공학 생성, 샷 분할(<= 8.0초), `refs/` 에셋 추출, `prompt.ko.md`/`prompt.h3.txt` 생성, Skip-by-default 동작, `run.sh` 생성이 모두 검증되어야 한다.

## 구현 결과

1. **Rust Cargo 프로젝트 및 CLI 아키텍처 (`rea-movie`) 구현 완료**:
   - `Cargo.toml` 및 바이너리 진입점(`src/main.rs`), CLI 모듈(`src/cli.rs`)을 구성하였습니다.
   - 첫 번째 인자로 영상 파일 경로가 주어지는 경우(`rea-movie <input> [OPTS]`) 자동으로 `decompile` 명령으로 정규화하는 전처리 로직(`normalize_cli_args`)을 구현하여 직관적인 기본 실행과 명시적 서브커맨드(`decompile`, `render`, `sync-prompt`, `assemble`, `pool`, `mcp`)를 모두 지원합니다.

2. **단일 카메라 컷 엄수 샷 분할 및 레퍼런스 추출 파이프라인 (`src/decompile/`)**:
   - `scene.rs`: `ffmpeg`의 `select='gt(scene,0.25)',showinfo` 필터를 통해 실제 카메라 컷 전환 지점들을 검출하고, 단일 컷이 `max_shot_duration`(기본 8.0초)을 초과하는 경우 8.0초 이하의 서브샷으로 균등 분할하며 `is_chained_longtake: true` 플래그를 설정하도록 구현했습니다. `--range START-END` 지정 파싱도 완벽 지원합니다.
   - `extract.rs`: 각 샷(`shot_XXXX`)에 대해 `refs/original_clip.mp4`, `refs/original_audio.wav`, 잔상 방지를 위한 시작 `+2프레임`(`first_frame.jpg`), 끝 `-2프레임`(`last_frame.jpg`), 그리고 빈 렌더 디렉토리(`renders/prod/`, `renders/smoke/`)를 생성합니다.
   - `contact_sheet.rs`: 시스템 폰트 의존성 없이 순수 Rust 비트맵 폰트 렌더러를 통해 각 1초 스냅샷 좌상단에 `[0.0s]`, `[1.0s]` 타임스탬프를 번인하고 가로 스트립으로 합성한 `refs/contact_sheet.jpg`를 생성합니다.
   - `audio.rs`: `hound` 크레이트를 사용해 PCM 16-bit 오디오를 분석하여 유의미한 데시벨 피크 타임스탬프(`audio_peaks`)를 추출합니다.

3. **2계층 프롬프트 생성 (`prompt.ko.md` & `prompt.h3.txt`) 및 멱등성(Skip-by-default)**:
   - `prompt.rs`: `prompt.ko.md`(화면/카메라, 초 단위 동작 궤적, 대사, 공간음/피크음, 배경음악) 및 MiniMax-H3 공식 스키마 영문 프롬프트 `prompt.h3.txt`(`integrated_multimodal_description:`, `overall_soundscape:`, `non_diegetic_music:`)를 생성합니다.
   - `MINIMAX_API_KEY` 설정 시 MiniMax-M3 비전 API 호출을 지원하며, 미설정 또는 `--offline` 시 결정론적 분석 템플릿으로 생성합니다.
   - 타겟 파일이 이미 존재하면 `--force` 플래그가 없는 한 덮어쓰지 않고 보존하는 Skip-by-default 멱등성을 완벽히 구현했습니다.

4. **프로젝트 설정 및 스토리/인물 에셋, 단일 진입점 `./run.sh`**:
   - `config.rs`: `rea-movie.toml`에 DGX Spark 서버 풀(`spark1.local`, `spark2.local`, `~/rea-movie-workspace`), `prod`/`smoke` 해상도 및 파라미터, MiniMax M3 설정 스키마를 구성했습니다.
   - `timeline.rs`: 전체 샷의 타임코드, 앵커 모드, 롱테이크 체인 플래그를 담은 `timeline.json` 및 개별 `shot.toml`을 직렬화/역직렬화합니다.
   - `story.rs`: `story/screenplay.md`, `story/characters.toml`(S1, S2 보이스 지문 포함), `story/transcript.srt`, `assets/characters/S1_narrator/face.jpg`, `voice.wav`를 자동 생성합니다.
   - `run_script.rs`: 실행 권한(`chmod +x`)을 가진 단일 진입점 `./run.sh`를 생성하여 인자 파싱(`--smoke`, 샷 번호/범위, `--force`, `--force-video-only`) 후 `rea-movie render` 및 `rea-movie assemble`을 연속 디스패치하도록 구성했습니다.

5. **검증 결과**:
   - `cargo test`: 인수 정규화, 시간 파싱, 범위 파싱, `rea-movie.toml` 및 `shot.toml` 직렬화 5개 테스트 전체 통과 (5 passed, 0 failed).
   - `cargo build --release`: 컴파일 경고 및 오류 없이 성공적으로 바이너리 생성 확인.
   - `./regression-tests/verify-issue-1.sh`: 12초 멀티씬 합성 테스트 영상으로부터 프로젝트 레이아웃, `rea-movie.toml`, 샷 분할(<= 8.0s), `refs/` 에셋 및 `contact_sheet.jpg` 추출, `prompt.ko.md`/`prompt.h3.txt` 필수 섹션 검출, Skip-by-default 멱등성 유지, 실행 가능한 `run.sh` 생성을 모두 검증하여 **`ALL CHECKS PASSED`** 확인 완료.
