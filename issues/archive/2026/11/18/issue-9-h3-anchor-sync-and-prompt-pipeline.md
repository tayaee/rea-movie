# Issue 9: H3 4.0초 미만 샷 앵커 보존 리타이밍 및 프롬프트 파이프라인 통합

## 1. 배경 및 문제

`src/render.rs`의 `shot.duration.clamp(4.0, 15.0)` 때문에 **4초보다 짧은 샷은 H3에
4초로 요청**된다. `frame_indices=[0,-1]`일 때 H3 렌더물의 `t=0.0s`는
`first_frame.jpg`이고 `t=render_dur` 끝단은 `last_frame.jpg`다.

기존 `src/assemble.rs`의 `trim_video_clip()`은 단순 `-t trim_dur` 절단만 수행하여
1.5~2.5초 샷의 **뒤쪽(last_frame 수렴 구간)을 통째로 잘라버리는 치명적 버그**가
있었다. 조립본에서는 시작 앵커만 남고 끝 앵커가 소실되어 원본 컷과 어긋난다.

추가로 M3 샷 프롬프트 생성기는 `contact_sheet.jpg`만 보고, 씬 바이블·시리즈
바이블·`<Picture 1>/<Picture 2>` 앵커를 직접 참조하지 않았다. H3 전송 프롬프트에는
`<!-- rea-movie:... -->` 멱등성 마커가 그대로 섞여 들어가고, 오프라인 폴백
컴파일러(`compile_ko_to_h3`)는 한국어 연출 서술문을 H3 영문 스키마 섹션에 그대로
복사했다.

## 2. 목표

1. 조립 시 렌더 클립은 **전 구간(0..render_dur)을 살려 `target_dur`에 리타이밍**하고,
   원본 클립은 기존처럼 `-t` 절단하여 원본 타이밍과 일치시킨다.
2. `--min-shot-duration` 옵션으로 미세 컷 병합을 지원한다 (기본값 0.0 = 병합 없음).
3. M3 샷 프롬프트 생성에 **씬 바이블 + 시리즈 바이블 + first/last 앵커 프레임**을
   직접 주입하고, 온라인 M3 출력의 `reference_line` 헤더를 정규화한다.
4. H3 전송 직전 HTML 주석 마커를 제거하고(디스크 파일은 마커 유지),
   오프라인 폴백 컴파일 출력의 비대사 영역에서 한글을 제거한다.

## 3. 상세 요구사항

### 3-1. 요구사항 A — 앵커 보존 리타이밍 (`assemble.rs`, `scene.rs`, `cli.rs`)

a) H3 서버는 최소 4.0초로 비디오를 생성하며, `frame_indices=[0,-1]`일 때
   `t=0.0s`가 `first_frame.jpg`, `t=4.0s`가 `last_frame.jpg`이다.
b) 기존 `trim_video_clip()`의 단순 절단은 1.5~2.5초 샷의 뒤쪽을 잘라내는 버그였다.
c) 렌더 클립 보정 시 `render_dur > target_dur + 0.08`인 경우:
   - `render_video`는 끝 프레임까지의 전 구간을 살려 `target_dur` 길이에 맞게 비디오
     타임스탬프 리타이밍 (`setpts=(target_dur/render_dur)*PTS`, `fps=...`),
     오디오는 `atempo` 체인 + `atrim=0:target` (+ `-t target`)으로 정확히 맞춘다.
     **주의**: `apad`는 `-preset veryfast` 조합에서 ffmpeg 데드락을 일으키므로
     사용하지 않는다 (실측 확인).
   - `orig_clip`은 기존처럼 `-t trim_dur` 절단한다 (원본 타이밍 일치).
d) `decompile` CLI 및 `build_shot_spans()`에 `--min-shot-duration <f64>` 옵션
   (기본값 0.0으로 기존 동작·테스트 호환 유지)을 추가하고, 지정 시 너무 짧은 미세
   컷들을 인접 샷과 병합한다 (`max_shot_duration` 이하 범위 내에서만).

### 3-2. 요구사항 B — M3 프롬프트 생성기 통합 (`prompt.rs`, `decompile/mod.rs`)

a) `PromptGenContext`에 `first_frame_path: Option<&Path>`,
   `last_frame_path: Option<&Path>`, `scene_bible: Option<&SceneBible>`,
   `series_world_summary: Option<&str>`, `anchor: &str`를 추가한다.
b) `try_minimax_m3_api()` 호출 시:
   - `contact_sheet.jpg`뿐 아니라 `first_frame.jpg`(`<Picture 1>`)와
     `last_frame.jpg`(`<Picture 2>`)가 존재하면 멀티모달 이미지로 함께 전달하고,
     `<Picture 1>` 초기 구도 → 중간 동작 → `<Picture 2>` 최종 구도의 정확한
     델타(Delta trajectory) 묘사를 지시한다.
   - `scene_bible`(장소·시간대·S1/S2 외모·의상·공간 배치·시선 문법·대사 흐름)과
     시리즈 컨텍스트(`<root>/story/world.json` 요약)를 user/system 메시지에 포함시켜
     샷 본문이 씬 바이블 인물 ID·시선 방향과 100% 일관되게 작성되도록 한다.
   - 온라인 M3의 `prompt_h3` 선두에 앵커 종류에 맞는 `reference_line` 헤더를
     `ensure_h3_reference_header()`로 정규화한다.
   - 방송국 로고(예: VA 로고)·워터마크·번인 타임스탬프·하드섭 자막 등 비주얼 노이즈
     묘사를 시스템 프롬프트 Negative rule로 명시 금지한다.

### 3-3. 요구사항 C — H3 전송 정리 (`render.rs`, `sync_prompt.rs`)

a) `src/render.rs`에서 H3 서버 전송 직전에
   `strip_html_comment_markers()` / `clean_prompt_for_h3()`를 호출하여
   `<!-- rea-movie:... -->` 라인들을 제거한 깨끗한 프롬프트만 전송한다.
   디스크의 `prompt.h3.txt`는 멱등성 유지를 위해 마커를 그대로 유지한다.
b) `src/sync_prompt.rs`의 `compile_ko_to_h3()` 오프라인 폴백에서 한국어 문장이
   `integrated_multimodal_description:` / `overall_soundscape:` /
   `non_diegetic_music:`에 그대로 복사되지 않도록 한다.
   (`<d>[Korean] ...</d>` 태그 내부 대사는 보존, 일반 연출 서술문 한글은 영문
   구조화 디렉션으로 변환/대체.)

## 4. 검수 기준

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
2. `regression-tests/verify-issue-9.sh` 통과:
   - 4.0초 렌더(전반 red / 후반 blue)를 2.0초 타겟으로 조립 시 전반·후반 색상 모두
     보존 (단순 절단이면 후반 색상 소실).
   - `ensure_h3_reference_header`·`clean_prompt_for_h3` 동작.
   - 한글 연출 노트 컴파일 시 비대사 영역 한글 0건 + `<Picture 1> and <Picture 2>`
     헤더 유지.
   - `--min-shot-duration` 옵션 동작.
3. 기존 `verify-issue-4.sh` ~ `verify-issue-8.sh` 모두 통과 (커밋 없음).

## 5. 구현 결과

### 5-1. `src/assemble.rs` — 렌더 리타이밍 vs 원본 절단 분리

- `atempo_chain_filter(ratio)`: tempo 배율을 `atempo=` 체인으로 분해
  (루프당 `2.0`/`0.5` 클램프, 소수점 6자리 고정).
- `retime_render_clip(input, out, target_dur)` (신규):
  `fps` 정렬(`frames = round(target*fps)/fps`) 후
  `-filter:v setpts=(trim/render)*PTS,fps=...`,
  `-filter:a <atempo 체인>,atrim=0:trim` + `-t trim`,
  `-c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -c:a aac -ar 32000 -ac 2`.
- `prepare_effective_shots()`: **렌더 클립은 `retime_render_clip()`**,
  **원본 클립은 기존 `trim_video_clip()`**으로 구분 처리
  (둘 다 임계값 `render_dur > target_dur + 0.08`, `.trim/` 재사용·조립 후 삭제 유지).
- 단위 테스트: `test_atempo_chain_covers_large_ratios`,
  `test_retime_render_clip_matches_target_duration` (4초→2초, 오차 ±0.25초).

### 5-2. `src/decompile/scene.rs` + `src/cli.rs` — 미세 컷 병합

- `build_shot_spans(.., min_shot_duration)` /
  `build_shot_spans_with(.., min_shot_duration, detect)` 시그니처 확장.
- `merge_short_spans(spans, min, max)` (신규, `pub`): `min` 미만 스팬을 다음
  스팬과 전진 병합 (합이 `max` 이하일 때만). 꼬리 잔류분은 마지막 스팬에 흡수
  시도 후 불가 시 단독 유지. 전체 `shot_XXXX` 재넘버링.
- `cli.rs` `DecompileArgs`에 `--min-shot-duration` (기본값 `0.0`) 추가,
  `main.rs`·`mcp.rs`·`decompile/mod.rs` 배선 완료.
- 단위 테스트: `test_merge_short_spans_noop_when_min_zero`,
  `test_merge_short_spans_combines_micro_cut`,
  `test_merge_short_spans_respects_max`,
  `test_build_shot_spans_with_min_merges`.

### 5-3. `src/decompile/prompt.rs` + `src/decompile/mod.rs` — 바이블·앵커 주입

- `PromptGenContext` 5개 필드 추가 (요구사항 B-a 그대로).
- `reference_line_for(anchor, shot_index)` (신규, `pub`): 앵커별 기대 헤더 생성.
- `ensure_h3_reference_header(h3, shot_index, anchor)` (신규, `pub`):
  일치 시 유지, 첫 줄이 `For the target video...`인데 내용이 다르면 교체,
  없으면 선두에 prepend.
- `try_minimax_m3_api()`:
  `contact_sheet` + `<Picture 1>` + `<Picture 2>` 멀티모달 전송,
  Delta-trajectory 지시문, scene bible + series 요약 본문 주입,
  Negative rule(로고·워터마크·번인 타임스탬프·하드섭 금지) 시스템 프롬프트,
  반환 `prompt_h3`에 `ensure_h3_reference_header` 적용.
- `translate_ko_base_to_h3_m3()`: 동일 Negative rule + 헤더 정규화 적용.
- `generate_deterministic_template()`: 앵커별 헤더 + Delta 문구 + Negative 문구 +
  series 요약행; `generate_prompts()`는 `ctx.scene_bible` 우선 사용.
- `decompile/mod.rs`: `load_series_world_summary()` (신규, `pub`) —
  `<root>/story/world.json` 또는 `<proj>/story/world.json`을 읽어
  `era/season/weather/color_grade/locations/cinematography` 요약(≤1200자).
  루프 B에서 앵커 프레임 경로·씬 바이블·시리즈 요약을 `PromptGenContext`에 주입.
- 단위 테스트: `test_reference_line_for_anchors`, `test_ensure_h3_reference_header`.

### 5-4. `src/render.rs` — 전송 전 마커 제거

- `strip_html_comment_markers()` (신규, `pub`): `rea-movie:` 포함 HTML 주석 구간
  제거 (비-rea-movie 주석은 보존).
- `clean_prompt_for_h3()` (신규, `pub`): 마커 제거 + 빈 줄 정리·양끝 trim.
- `request_h3()`는 전송 `prompt`에만 `clean_prompt_for_h3()` 적용
  (디스크 파일은 마커 유지).
- 단위 테스트: `strip_markers_removes_rea_movie_comments_only`.

### 5-5. `src/sync_prompt.rs` — 오프라인 한글 필터

- `contains_hangul_outside_dialogue()` (신규, `pub`): `<d>...</d>` 구간을 제외하고
  한글(완성형·자모·호환자모) 검출.
- `strip_scene_block_for_compile()`: 컴파일 입력에서 씬 바이블 블록 제거
  (영문 씬 블록은 `compile_prompt_file`이 나중에 재첨부).
- `clean_h3_body_lines()`: `<d>` 라인 보존, 마커 라인 제거, 한글 서술문은
  섹션별 영문 폴백 1줄로 대체 (`대사 없음` → 무음 영문 문장).
- `compile_ko_to_h3()`: 세 섹션 모두 위 필터 적용. 기존 버그
  (`integrated` 비어 있으면 `ko_prompt` 원문=한글 그대로 복사) 수정 →
  영문 폴백 사용.
- 단위 테스트: `compiler_drops_korean_prose_but_keeps_dialogue`
  (헤더·`<d>` 보존 + 3개 섹션 비대사 한글 0건 검증).

### 5-6. 회귀 검증 (`regression-tests/verify-issue-9.sh`, 실행 권한 부여)

- Check 1: 4.0초 렌더(2초 red + 2초 blue, sine 오디오)를 2.0초 타겟으로 조립 →
  길이 2.0±0.5초, `t=0.2s` 프레임 적색 우세·`t=1.8s` 프레임 청색 우세
  (rawvideo 32×32 평균 RGB), `.trim/` 정리 확인.
- Check 2: `cargo test`로 header/marker/Korean 단위 테스트 + 마커 픽스처 존재 확인.
- Check 3: `sync-prompt --force` 종단 검증 — 한글 `prompt.ko.md` →
  `prompt.h3.txt`에 `<Picture 1> and <Picture 2> (from [Shot 1])` 헤더,
  `<d>[Korean] ...</d>` 보존, 3개 섹션 비대사 한글 0건 (정규식으로 `<d>` 제거 후 검사).
- Check 4: `decompile --help`에 `--min-shot-duration` 존재,
  `merge_short_spans`/`build_shot_spans_with_min` 단위 테스트,
  4초 hard-cut 영상 decompile 종단 비교 (merged ≤ plain).

### 5-7. 품질 검사 통과

- `cargo fmt --check`: 통과 (신규 코드 포함 전체 포맷 적용).
- `cargo clippy --all-targets -- -D warnings`: 경고 0건.
- `cargo test`: 전체 73개 단위 테스트 통과 (신규 12개 포함).
- `regression-tests/verify-issue-9.sh`: 전체 4개 체크 패스.
- 기존 `verify-issue-4.sh` ~ `verify-issue-8.sh`: 모두 통과 (아래 검증 로그 참조).
