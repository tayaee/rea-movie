# Issue 8: sources/ 캐시 재사용 아키텍처 및 series-watch 선행 분석 파이프라인

## 1. 개요 및 배경

`rea-movie`를 통한 영상 역공학 과정에서 동일한 소스(로컬 비디오 파일 또는 YouTube 단일 영상/플레이리스트)를 대상으로 다양한 실험을 수행합니다:
- 다른 time range (`--range 0-60`, `--range 60-120`)
- 연출서 재작성 및 다른 프로젝트 버전 (`-v1`, `-v2`, `-v3`)
- 다른 더빙 언어 및 프롬프트 커스터마이징

기존 방식에서는 매번:
1. `yt-dlp`로 영상을 중복 다운로드하고 `.vtt`를 `.srt`로 중복 변환
2. 매우 느린 `ffmpeg select='gt(scene,0.14)'` scene cut 감지를 처음부터 재실행
3. 동일한 타임스탬프의 `first_frame`, `last_frame`, `original_clip`, `original_audio`, `contact_sheet`를 매번 ffmpeg으로 중복 추출
4. 시리즈 전체 맥락(`story/world.json`, `story/characters.json`, `story/timeline-days.md`) 없이 개별 decompile이 실행되어 'Warning: no series context' 경고 발생

본 이슈는 **재사용 가능한 모든 에셋을 `<root>/sources/<source_id>/`에 1회만 추출/캐시하여 100% 재사용**하고, 풀 decompile 전에 시리즈 전편을 가볍게 선행 분석하는 **`rea-movie series-watch` 파이프라인**을 구축하는 것을 목표로 합니다.

---

## 2. 핵심 요구사항 및 아키텍처 설계

### 2-1. `<root>/sources/` 영구 캐시 디렉토리 레이아웃

```text
<output-root>/
├── sources/
│   ├── index.json                             # 입력 경로/크기/mtime/URL 매핑 인덱스
│   ├── playlists/                             # 플레이리스트 영상 ID 목록 캐시 (.tsv)
│   └── <source_id>/                           # yt-<id> 또는 file-<fingerprint>
│       ├── video.mp4 (또는 심볼릭 링크)        # 원본 비디오 참조
│       ├── probe.json                         # ffprobe 메타데이터 (duration, fps, width, height 등)
│       ├── scene_cuts.json                    # ffmpeg gt(scene,0.14) 컷 감지 결과 (구간 병합 지원)
│       ├── subtitles/                         # 추출/변환된 자막 캐시 (*.srt)
│       │   ├── embedded.srt (또는 *.ko.srt)
│       │   └── .no-embedded                   # 임베디드 자막 부재 시 재시도 방지 마커
│       ├── keyframes/                         # 5초 간격 프로브 프레임 및 타임스탬프 앵커 프레임
│       │   ├── probe_t000000000.jpg (480px)
│       │   └── anchor_t000000080.jpg (원본 해상도)
│       └── shots_cache/<start_ms>_<end_ms>/   # 샷 구간별 캐시 에셋
│           ├── original_clip.mp4
│           ├── original_audio.wav
│           └── contact_sheet.jpg
├── story/                                     # series-watch 가 생성하는 전역 시리즈 맥락
│   ├── world.json
│   ├── characters.json
│   └── timeline-days.md
└── h3-projects/<project_name>/                # 프로젝트별 가벼운 연출서 및 작업 공간
```

- **구 레이아웃 호환**: 기존 `<output-root>/source/`에 다운로드된 영상도 감지하여 자동 인식/심볼릭 링크하여 폴백 지원.

### 2-2. 정규화된 `source_id` 및 `index.json`
- **YouTube 영상**: 파일명이나 URL의 11자리 ID 패턴 `[<id>]`에서 추출하여 `yt-<video_id>`로 일관성 부여.
- **로컬 파일**: 파일 크기 + 앞/뒤 1MiB 청크 기반의 고속 FNV-1a 64비트 해시(`file-<fingerprint>`)로 경로가 변경되거나 복사되어도 동일 파일 인식.
- **Fast lookup**: `index.json`에 `(path, size, mtime)`을 인덱싱하여 빠른 매핑 지원.

### 2-3. Scene Cut 및 샷 에셋 캐시 (`scene_cuts.json` & `shots_cache/`)
- `scene_cuts.json`: 비디오 전체 또는 특정 구간(`start-end`)에 대해 감지된 컷 목록을 기록하며, 중복 구간은 자동 병합(`insert`). 다른 `--range`나 다른 프로젝트로 재디컴파일 시 기스캔 구간에 완전히 포함되면 느린 ffmpeg 씬 감지를 0초에 건너뜀.
- `shots_cache/`: 구간 시작/종료 밀리초 키(`000000000_000003500`)로 `original_clip.mp4`, `original_audio.wav`, `contact_sheet.jpg`를 보관하며, `keyframes/`에 `anchor_t<ms>.jpg`를 보관하여 `--force`가 아니면 즉시 원자적 복사 재사용.

### 2-4. `rea-movie series-watch` CLI 및 MCP 도구
- 전체 에피소드를 풀 decompile하지 않고 가벼운 프로브(ffprobe + 5초 간격 샘플 키프레임 + 자막 파싱)만 수행.
- 에피소드 비디오를 자연어 순서(EP1, EP2, ... EP10)로 정렬.
- 자막 화자 태그(`WALTER:`, `[S1]`, `(지수)`) 및 색상 팔레트/루마 분석을 기반으로 결정적(deterministic) 오프라인 기본 컨텍스트 구축.
- `MINIMAX_API_KEY`가 주어지면 MiniMax-M3 1회 호출로 시리즈 전체를 요약하여 세계관/캐릭터/타임라인 초안 생성 (실패 시 오프라인 모드로 안전 폴백).
- 이미 `story/world.json`, `story/characters.json`, `story/timeline-days.md`가 존재하면 `--force`가 아닐 때 재생성 스킵.
- MCP 도구 `series_watch`로 AI 에이전트 환경에서 도구 호출 가능.

### 2-5. `scripts/reverse-url.sh` 개선
- 다운로드 대상 폴더를 `<output-root>/sources/yt-<id>/`로 변경.
- 이미 캐시에 영상이 있으면 yt-dlp 다운로드를 건너뛰고 재사용 메시지 출력.
- `.vtt` 자막 변환 시 `.srt`가 이미 존재하면 변환 스킵.
- 플레이리스트인 경우 decompile 전에 먼저 `rea-movie series-watch`를 실행하여 `<root>/story/`를 선행 확보.
- `--episodes 1,2` 또는 `1-3` 옵션을 지원하여 원하는 에피소드만 풀 decompile할 수 있도록 제어.

---

## 3. 구현 내용 요약

1. **`src/sources.rs` 신규 구현**:
   - `SourceIndex`, `IndexedInput`, `IndexEntry`: `index.json` 직렬화 및 고속 조회
   - `ProbeInfo`, `run_ffprobe`: ffprobe 메타데이터 캐싱
   - `SceneCutsCache`, `CutSegment`: 씬 감지 타임스탬프 캐시 및 구간 병합
   - `SourceCache`: 소스별 디렉토리 준비, 심볼릭 링크, 키프레임/자막/샷 에셋 캐시 관리
   - `materialize_asset`: 원자적 캐시 복사 및 신규 생성 에셋 캐싱
   - `pick_srt`: 언어 우선순위(`ko` > plain > `en` > `diarized` > `embedded`)에 따른 최적 자막 선택
2. **`src/series_watch.rs` 신규 구현**:
   - `run_series_watch`: 에피소드 스캔, 프로브, 5초 키프레임 추출, 자막 수집, 팔레트 분석
   - `deterministic_world`, `deterministic_characters`, `deterministic_timeline_md`: 결정적 시리즈 메타데이터 생성
   - `summarize_with_m3`: MiniMax-M3 1회 호출 및 안전한 fallback
3. **CLI 및 MCP 서버 연동**:
   - `src/cli.rs`: `SeriesWatchArgs` 추가 (`rea-movie series-watch`)
   - `src/main.rs`: CLI 서브커맨드 라우팅 및 `normalize_cli_args`에 `series-watch` 추가
   - `src/mcp.rs`: `series_watch` MCP 도구 스키마 및 실행 핸들러 등록
4. **`src/decompile/` 파이프라인 연동**:
   - `extract.rs`: `extract_shot_references`에서 `SourceCache`를 통한 샷 에셋(클립, 오디오, 앵커 프레임, 콘택트시트) 재사용
   - `scene.rs`: `build_shot_spans_with`를 통해 캐시된 `scene_cuts.json` 재사용 지원
   - `story.rs`: `load_subtitle_cues_from` 및 `subtitle_srt`를 통한 캐시된 자막 사용
   - `mod.rs`: `resolve_sources_dir` 및 `SourceCache` 생성, 캐시 재사용 통계 출력
5. **`scripts/reverse-url.sh` 스크립트 고도화**:
   - `sources/yt-<id>` 기반 캐싱 및 yt-dlp 중복 다운로드 방지
   - `series-watch` 선행 실행 및 `--episodes` 옵션 지원

---

## 4. 검증 결과

1. **단위 테스트 (Unit Tests)**:
   - `cargo test` 실행 결과 63개 테스트 모두 통과.
   - `sources::tests`: `test_youtube_id_from_name`, `test_root_from_input_new_and_legacy`, `test_scene_cuts_cache_lookup_and_merge`, `test_pick_srt_priority`, `test_open_index_and_materialize_reuse`
   - `series_watch::tests`: `test_natural_key_orders_episode_numbers`, `test_speaker_tag_detection`, `test_subtitle_excerpt_respects_budget`
2. **코드 스타일 및 린트**:
   - `cargo fmt --check`: 통과
   - `cargo clippy --all-targets -- -D warnings`: 경고 0건으로 통과
3. **기존 회귀 테스트**:
   - `verify-issue-4.sh`: 통과 (Customization layer & character traits)
   - `verify-issue-5.sh`: 통과 (Anchor frame prop edit & MCP customize_anchor)
   - `verify-issue-6.sh`: 통과 (Assembly shot duration sync & trim)
4. **신규 회귀 테스트 (`regression-tests/verify-issue-8.sh`)**:
   - 검증 1: 동일 비디오 연속 decompile 시 `sources/index.json`, `probe.json`, `scene_cuts.json`, `keyframes/`, `shots_cache/` 생성 및 두 번째 decompile에서 100% 캐시 재사용 검증.
   - 검증 2: `rea-movie series-watch` 실행 후 `story/world.json`, `story/characters.json`, `story/timeline-days.md` 정상 생성 검증 및 후속 decompile 시 'Warning: no series context' 경고가 발생하지 않음을 검증.
   - 검증 3: `scripts/reverse-url.sh`의 캐시 디렉토리 인식, yt-dlp 다운로드 스킵, `--episodes` 필터링 및 `series-watch` 선행 파이프라인 검증.
