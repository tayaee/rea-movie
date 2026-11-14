# Issue 3: 최종 영화 조립기(`assemble`), 좌우 1:1 동시 비교 영상(`_compare.mp4`), MCP 서버 및 `/rea-movie` 스킬 구현

## 1. 배경 및 목표
렌더링된 샷들을 이어붙여 듀얼 오디오 트랙(Track 1: H3 생성 오디오, Track 2: 원본 영화 오디오)을 가진 최종 영화 파일을 생성하고, **[좌측: 원본 영화 | 우측: H3 재현 영화 + `Shot #XXXX` 라벨]**이 나란히 재생되는 1:1 동시 비교 영상을 자동 생성한다. 또한 AI 에이전트가 프로젝트를 제어할 수 있도록 `rea-movie mcp` JSON-RPC 2.0 stdio 서버와 `/rea-movie` 스킬 문서를 완성한다.

## 2. 상세 요구사항

1. **영화 조립기 및 1:1 동시 비교 영상 생성 (`src/assemble.rs`)**:
   - `rea-movie assemble` (기본 Prod: `outputs/prod/`) 및 `rea-movie assemble --smoke` (Smoke: `outputs/smoke/`) 지원.
   - `./run.sh` 실행 시 렌더링 단계 직후 자동으로 `assemble`까지 수행되어야 한다.
   - **최종 본편 영상 (`outputs/<profile>/<movie_name>_<profile>.mp4`)**:
     - 각 샷의 `renders/<profile>/shot_XXXX.mp4`를 순서대로 이어붙인다.
     - 오디오 스트림 0번(Track 1)은 H3가 생성한 스테레오 오디오, 오디오 스트림 1번(Track 2)은 원본 영화의 오디오 구간을 함께 먹싱(Muxing)하여 플레이어에서 오디오 트랙을 전환하며 비교할 수 있게 한다.
   - **1:1 좌우 동시 비교 영상 (`outputs/<profile>/<movie_name>_compare.mp4`)**:
     - 각 샷의 `refs/original_clip.mp4`(좌측)와 `renders/<profile>/shot_XXXX.mp4`(우측)를 동일 높이로 스케일링 후 `hstack`으로 나란히 배치하여 병합한다 (우측 상단 또는 하단에 `Shot #XXXX` 식별 가능하도록 구성, `drawtext` 폰트 미설치 환경에서도 실패하지 않도록 안전한 필터 또는 폴백 처리).

2. **MCP 서버 (`rea-movie mcp`, `src/mcp.rs`)**:
   - 표준 MCP JSON-RPC 2.0 over stdio 프로토콜 구현 (`initialize`, `notifications/initialized`, `tools/list`, `tools/call`).
   - 제공 툴:
     - `decompile_movie`: 입력 영상 역공학 프로젝트 생성
     - `list_shots`: 프로젝트 내 샷 목록 및 렌더링 상태 조회
     - `inspect_shot`: 특정 샷의 `shot.toml`, `prompt.ko.md`, `prompt.h3.txt` 조회
     - `update_shot_prompt`: 특정 샷의 `prompt.ko.md`, `prompt.h3.txt`, 또는 `anchor` 설정 업데이트
     - `render_shot`: 특정 샷 또는 범위 렌더링 실행 (`prod` / `smoke`, `force` 지원)
     - `assemble_movie`: 전체 영화 및 비교 영상 조립
     - `pool_status`: DGX Spark 서버 풀 상태 조회

3. **`/rea-movie` 스킬 (`skills/rea-movie/SKILL.md`)**:
   - 에이전트가 `/rea-movie` 스킬을 로드했을 때 `rea-movie` CLI와 `run.sh`, 부분 샷 편집(`vi shots/shot_0005/prompt.ko.md` -> `./run.sh 0005 --force`), DGX Spark 서버 풀 제어 및 MCP 도구를 완벽히 사용할 수 있도록 안내하는 `skills/rea-movie/SKILL.md` 작성 (및 `~/.gemini/config/skills/rea-movie/SKILL.md`에 심볼릭 링크/복사).

## 3. 검수 기준
- `cargo test` 통과 및 `./regression-tests/verify-issue-3.sh` 통과.

## 구현 결과

1. **영화 조립기 및 1:1 동시 비교 영상 생성 (`src/assemble.rs`) 구현**:
   - `rea-movie assemble` (기본 Prod: `outputs/prod/`) 및 `rea-movie assemble --smoke` (Smoke: `outputs/smoke/`) 완벽 지원.
   - 단일 진입점 `./run.sh`에서 렌더링 직후 `assemble`을 자동 호출하여 최종 영상들을 생성하도록 연동.
   - **최종 본편 영상 (`outputs/<profile>/<movie_name>_<profile>.mp4`)**:
     - 렌더링된 각 샷(`renders/<profile>/shot_XXXX.mp4`)을 순서대로 concat demuxer로 결합.
     - 오디오 스트림 0번(Track 1: `H3 Generated Audio`)과 오디오 스트림 1번(Track 2: `Original Movie Audio`)을 듀얼 먹싱(Muxing)하여 미디어 플레이어에서 오디오 트랙을 전환하며 청취 가능하도록 구성.
   - **1:1 좌우 동시 비교 영상 (`outputs/<profile>/<movie_name>_compare.mp4`)**:
     - 샷별로 원본 클립(`refs/original_clip.mp4`)과 H3 렌더본(`renders/<profile>/shot_XXXX.mp4`)을 동일 높이로 스케일링 후 `hstack`으로 병합.
     - 우측 상단에 `Shot #XXXX` 라벨을 오버레이. 시스템 폰트 자동 탐색 및 `drawtext` 실패 시 라벨 없는 `hstack`으로 자동 폴백하는 안전망 구현.
     - 경로 정규화(`canonicalize`)를 적용하여 ffmpeg concat demuxer의 상대 경로 오류 방지.

2. **MCP 서버 (`src/mcp.rs`) 구현**:
   - 표준 MCP JSON-RPC 2.0 over stdio 프로토콜(`initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`) 완벽 구현.
   - 요구 명세의 7대 도구 완비:
     - `decompile_movie`: 원본 영상 역공학 프로젝트 생성
     - `list_shots`: 프로젝트 내 샷 목록, 시간 구간, 앵커, Prod/Smoke 렌더링 상태 조회
     - `inspect_shot`: 특정 샷의 `shot.toml`, `prompt.ko.md`, `prompt.h3.txt` 내용 조회
     - `update_shot_prompt`: 한국어 연출서, H3 프롬프트, 앵커 모드 수정 및 `prompt.h3.txt` 자동 재컴파일
     - `render_shot`: 특정 샷/범위 렌더링 디스패치
     - `assemble_movie`: 전체 영화 및 1:1 비교 영상 조립 디스패치
     - `pool_status`: DGX Spark 서버 풀 노드, 포트 및 활성 엔드포인트 상태 조회

3. **`/rea-movie` 스킬 (`skills/rea-movie/SKILL.md`) 작성 및 환경 등록**:
   - `skills/rea-movie/SKILL.md` 작성 및 `~/.gemini/config/skills/rea-movie/SKILL.md`에 복사 배포 완료.
   - CLI 명령어, 단일 진입점 `./run.sh`, 연출 수정 루프(Director's editing loop), DGX Spark 풀 제어, MCP 도구 레퍼런스 및 핵심 엔지니어링 규칙을 체계적으로 기술.

4. **테스트 및 검증 결과**:
   - `cargo test`: 전체 18개 단위 테스트 통과 (`18 passed; 0 failed`).
   - `./regression-tests/verify-issue-3.sh`:
     - 1. Prod 기본 decompile 및 `./run.sh` 실행 (`outputs/prod/robocop_prod.mp4`, `outputs/prod/robocop_compare.mp4` 생성 확인)
     - 2. Smoke 격리 `./run.sh --smoke` 실행 (`outputs/smoke/robocop_smoke.mp4`, `outputs/smoke/robocop_compare.mp4` 분리 생성 확인)
     - 3. 특정 샷 재생성 및 히스토리 백업 `./run.sh 0001 --force` (`history/take_001.mp4` 생성 확인)
     - 4. MCP 서버 `tools/list` 조회 검증 (`decompile_movie`, `render_shot`, `assemble_movie` 등 확인)
     - 5. `/rea-movie` 스킬 파일 확인
     - 결과: **`ALL CHECKS PASSED`** 확인 완료.
   - 기존 회귀 테스트 `./regression-tests/verify-issue-1.sh` 및 `./regression-tests/verify-issue-2.sh`도 모두 `ALL CHECKS PASSED` 유지.
