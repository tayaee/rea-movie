# Issue 13: `source/` → `input/` 및 `sources/` → `input-details/` 디렉토리 네이밍 전환

## 1. 배경 및 목표

기존 디렉토리 구조에서 원본 입력 영상 폴더(`source/`)와 소스별 분석/추출 에셋 캐시 폴더(`sources/`), 그리고 Rust 소스 코드 폴더(`src/`)의 이름이 서로 비슷하여 혼동을 유발한다.

역할이 직관적으로 드러나도록 디렉토리 명칭을 다음과 같이 변경하여 저장·참조하도록 코드와 스크립트, 문서, 회귀 테스트를 일괄 개편한다:

- 기존 `source/` → **`input/`** (순수 입력 원본 비디오 디렉토리)
- 기존 `sources/` → **`input-details/`** (원본 영상별 `index.json`, `probe.json`, `scene_cuts.json`, `keyframes/`, `shots_cache/`, `subtitles/` 등 추출/분석 상세 캐시 디렉토리)
- 하위 호환: 기존에 이미 생성된 `<root>/sources/` 또는 `<root>/source/` 경로의 입력 파일이 주어졌을 때도 시리즈 루트(`root_from_input`) 및 `series-watch` 비디오 탐색(`collect_videos`)은 폴백으로 인식하되, **신규 생성 및 저장 디렉토리는 무조건 `<root>/input-details/`와 `<root>/input/`을 사용**한다.

---

## 2. 상세 요구사항

### 2-1. `src/sources.rs` 수정
1. 상수 변경 및 하위 호환 상수 정의:
   - `pub const SOURCES_DIR_NAME: &str = "input-details";` (신규 기본 상세 캐시 폴더명)
   - `pub const LEGACY_SOURCE_DIR_NAME: &str = "input";` (신규 기본 원본 입력 폴더명)
   - 구 명칭 호환 상수: `pub const OLD_SOURCES_DIR_NAME: &str = "sources";`, `pub const OLD_LEGACY_SOURCE_DIR_NAME: &str = "source";`
2. `root_from_input(input: &Path)`:
   - 상위 2단계 부모 디렉토리 이름이 `"input-details"`, `"input"`, `"sources"`, `"source"` 중 하나이면 그 부모 디렉토리를 시리즈 `<root>`로 반환.
3. `resolve_sources_dir(proj_dir: Option<&Path>, input: &Path)`:
   - `REA_MOVIE_SOURCES_DIR` 환경변수가 없으면 `<root>/input-details` (`root.join(SOURCES_DIR_NAME)`)를 반환하여 모든 캐시가 `input-details/` 아래에 저장되도록 함.
4. 단위 테스트(`test_root_from_input_new_and_legacy`, `test_open_index_and_materialize_reuse`):
   - `/r/input-details/yt-abc/ep.mp4`, `/r/input/ep.mp4` 및 레거시 `/r/sources/yt-abc/ep.mp4`, `/r/source/ep.mp4` 모두 `Some(PathBuf::from("/r"))`을 반환하는지 검증.
   - `test_open_index_and_materialize_reuse`에서 `root.path().join("input-details")`를 사용하도록 갱신.

### 2-2. `src/series_watch.rs` 수정
1. `resolve_root()`:
   - `--source-dir`의 디렉토리명이 `"input-details"`, `"input"`, `"sources"`, `"source"` 중 하나이면 부모 디렉토리를 `<root>`로 인식.
2. `collect_videos()`:
   - 명시적 입력/디렉토리가 없을 때 기본 탐색 대상에 `root.join("input-details")`, `root.join("input")` (및 폴백 `root.join("sources")`, `root.join("source")`)을 포함.
3. 에러 메시지 및 주석에서 `<root>/input-details/` 표기 반영.
4. `run_series_watch()`에서 `SourceCache::open`에 전달하는 캐시 디렉토리는 `root.join(sources::SOURCES_DIR_NAME)` (`<root>/input-details`)를 사용.

### 2-3. `scripts/reverse-url.sh` 수정
1. 디렉토리 변수 변경:
   - `SRC_DIR="${ROOT}/input-details"`
   - `LEGACY_SRC_DIR="${ROOT}/input"`
   - 구버전 폴백: `${ROOT}/input`뿐 아니라 `${ROOT}/source`나 `${ROOT}/sources/yt-${vid}`에 기존 파일이 있을 때도 재사용 가능하도록 지원하되, 신규 다운로드 및 캐시 저장은 `${ROOT}/input-details/yt-${vid}`에 저장.
2. 주석 및 완료 로그 출력에 `input-details/` 및 `input/` 반영.

### 2-4. 문서 및 스킬 동기화 (`skills/rea-movie/SKILL.md`, `README.md`)
1. `skills/rea-movie/SKILL.md`와 `README.md`에서 `<root>/sources/` → `<root>/input-details/`, `<root>/source/` → `<root>/input/`으로 디렉토리 레이아웃 및 설명 업데이트.
2. `/home/user1/.gemini/config/skills/rea-movie/SKILL.md`가 존재하면 `skills/rea-movie/SKILL.md`와 동일하게 동기화(`cp`).

### 2-5. 회귀 테스트 업데이트 및 신규 검증 (`regression-tests/verify-issue-8.sh`, `verify-issue-12.sh`, `verify-issue-13.sh`)
1. 기존 `regression-tests/verify-issue-8.sh` 및 `regression-tests/verify-issue-12.sh`가 새 디렉토리 이름(`input-details/`, `input/`) 기준으로 통과하도록 업데이트.
2. 신규 `regression-tests/verify-issue-13.sh` (`chmod +x`) 작성:
   - Check 1: `decompile` 실행 시 `<root>/input-details/index.json` 및 `<root>/input-details/<source_id>/{probe.json,scene_cuts.json,keyframes/,shots_cache/}`에 저장되고 2차 decompile에서 100% 재사용되는지 검증.
   - Check 2: `<root>/input/` 아래의 비디오로 `series-watch` 및 `decompile` 실행 시 `<root>`를 정상 인식하고 `<root>/input-details/`에 상세 캐시를 생성하는지 검증.
   - Check 3: `scripts/reverse-url.sh`, `skills/rea-movie/SKILL.md`, `README.md`에 `input-details` 및 `input`이 반영되어 있고, `verify-issue-8.sh` 및 `verify-issue-12.sh`도 모두 통과하는지 검증.

---

## 3. 검수 기준

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
2. `regression-tests/verify-issue-13.sh` 통과.
3. 기존 `regression-tests/verify-issue-8.sh`, `regression-tests/verify-issue-12.sh` 통과.
4. 구현 완료 후 본 문서 하단에 `## 구현 결과` 섹션 작성 (`git commit` / `git mv`는 오케스트레이터가 수행하므로 직접 실행 금지).

---

## 구현 결과

### 변경 파일
- `src/sources.rs`: `SOURCES_DIR_NAME="input-details"`, `LEGACY_SOURCE_DIR_NAME="input"`,
  호환 상수 `OLD_SOURCES_DIR_NAME="sources"` / `OLD_LEGACY_SOURCE_DIR_NAME="source"` 추가.
  `root_from_input()`이 4개 이름(`input-details`, `input`, `sources`, `source`)을 모두 인식하도록 확장,
  `resolve_sources_dir()`은 항상 `<root>/input-details` 반환. 단위 테스트
  `test_root_from_input_new_and_legacy`(신규 2건 + 레거시 2건) 및
  `test_open_index_and_materialize_reuse`(`input-details` 경로) 갱신. 모듈/함수 주석의
  `<root>/sources/` 표기를 `<root>/input-details/`(순수 입력은 `<root>/input/`)로 정리.
- `src/series_watch.rs`: `resolve_root()`이 4개 디렉토리명을 모두 `<root>`로 인식,
  `collect_videos()` 기본 탐색 대상에 `input-details`·`input` + 폴백 `sources`·`source` 포함,
  에러 메시지 `<root>/input-details/` 표기. `run_series_watch()`는 기존 코드 그대로
  `root.join(sources::SOURCES_DIR_NAME)` → `<root>/input-details` 사용.
- `src/cli.rs`(`--source-dir` 도움말), `src/mcp.rs`(`series_watch` 스키마 설명),
  `src/decompile/{mod,extract,scene,story}.rs`·`src/main.rs` 주석: `sources/` → `input-details/` 정리.
- `scripts/reverse-url.sh`: `SRC_DIR="${ROOT}/input-details"`, `LEGACY_SRC_DIR="${ROOT}/input"`,
  `OLD_SRC_DIR="${ROOT}/sources"`·`OLD_LEGACY_SRC_DIR="${ROOT}/source"` 추가.
  캐시 확인 순서 `input-details/yt-<id>` → 레거시 `sources/yt-<id>`(링크 복사) → 레거시
  `input/`·`source/`의 `*[<id>].mp4`(링크 복사) → 신규 다운로드(`input-details/yt-<id>`).
  완료 로그 `Input details:` / `Input:` 표기.
- `skills/rea-movie/SKILL.md`, `README.md`: `<root>/sources/` → `<root>/input-details/`,
  `<root>/source/` → `<root>/input/` 레이아웃·설명 갱신(단 `source_id`·`source_dir` 파라미터명은 유지),
  README에 레거시 자동 인식 문구 추가. `/home/user1/.gemini/config/skills/rea-movie/SKILL.md`에 `cp` 동기화.
- `regression-tests/verify-issue-8.sh`·`verify-issue-12.sh`: 입력 영상은 `<root>/input/`,
  캐시는 `<root>/input-details/` 기준으로 갱신(플레이리스트 사전 시드 = `input-details/yt-*`,
  `series-watch --source-dir <root>/input`, 정적 키워드 검사 `input-details/yt-`·`input-details/`).
- `regression-tests/verify-issue-13.sh`(신규, `chmod +x`):
  Check 1 = `input-details/` 캐시 생성 + 2차 100% 재사용(`reused, 0 extracted`) + `sources/` 미생성 검증,
  Check 2 = `input/` 기반 series-watch/decompile + 레거시 `sources/`·`source/` 폴백 인식 및
  신규 캐시 `input-details/` 착륙 검증,
  Check 3 = 스크립트·문서 키워드 + `verify-issue-8.sh`·`verify-issue-12.sh` 실행 검증.

### 검증 결과
- `cargo fmt --check`: 통과.
- `cargo clippy --all-targets -- -D warnings`: 통과(경고 0).
- `cargo test`: 87 passed, 0 failed (신규 `test_root_from_input_new_and_legacy` 포함).
- `regression-tests/verify-issue-13.sh`: ALL CHECKS PASSED (내부에서 issue-8·issue-12 스위트 실행 포함).
- `git commit`/`git mv` 미실행 (오케스트레이터 수행 예정).
