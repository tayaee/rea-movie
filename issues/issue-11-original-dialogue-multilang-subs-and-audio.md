# Issue 11: 대사 원문 재현(§3-2), 6개국어 외부 자막(.srt) 및 원본 오디오 기본 트랙(§3-4), 증명용 출력(§3-3)

## 1. 요구사항 (Issue 7 상세 spec 인용)

- §3-2 대사 원문 그대로 재현:
  1. 자막 큐는 시작점이 속한 샷 하나에만 할당 (`assign_cues_to_spans`), 조립본 대사 중복 0건.
  2. 대사는 **원본 그대로 재현**. 번역·의역·창작 금지. `<d>` 태그 언어 표기는 본문 언어와 일치.
     (종전 한국어 더빙 기본값·per-shot EN→KO 번역 방침 폐기.)
  3. 무음샷 h3 태그 영어 하드코딩을 `dub_lang` 추종으로 수정 (미해결 → 본 이슈에서 일관 무음 지시문으로 정리).
- §3-3 증명용 출력 vs 최종 출력:
  1. 증명용: 생성 비디오 자막 앞에 인물명을 한국어로 (`[이름] 영어대사` 1줄 통합). 인물 감지 증거.
  2. 증명용: 자막을 영어로 변환. 대사 감지 증거.
  3. 증명용(화자태그 `[이름] 대사` 자막)은 별도 번인 빌드로 리뷰.
  4. 최종본: 화자 태그 없음. 원본 언어 유지 가능.
- §3-4 다국어 자막 외부 파일 + 원본 언어 음성:
  1. 자막은 mux가 아니라 **외부 자막 파일 6개** (ko/zh/ja/en/fr/de `.srt`, 비디오별).
  2. 번역 원문은 §3-2 원문 대사. 번역 품질은 M3 배치 번역 + 캐시.
     (종전 per-shot 더빙 번역은 자막 파일 생성 파이프라인으로 이동.)
  3. 음성은 **원본 비디오 언어**를 따른다. 최종 본편 기본 오디오는 원본 오디오이며,
     H3 생성 오디오와의 관계(듀얼트랙 유지 vs 원본 단일)는 구현 시 확정 → 본 이슈는 듀얼트랙 유지.

## 2. 구현 결과

### 요구사항 A: 대사 원문 재현 + `<d>[Language]</d>` 자동 일치
- `src/decompile/prompt.rs`:
  - `detect_dialogue_language(text, dub_lang_hint)` 신규: Hangul(AC00–D7A3/Jamo/Compat)→Korean,
    히라가나/가타카나(3040–30FF)→Japanese, 한자(4E00–9FFF)→Chinese,
    `ß`/`ä·ö·ü`→German, 프랑스 악센트(`é/è/ê/ë/à/â/ç/î/ï/ô/œ/ù/û/ÿ/æ`)→French,
    라틴→English, 그 외 `dub_lang_hint` 폴백 (`dub_lang_to_language()`).
  - `build_dialogue_lines()`: 기본 원문 verbatim 유지. `--translate-dub`일 때만 기존
    `translate_line_to_korean()` 테이블 번역을 적용. `<d>` 태그는 `detect_dialogue_language()` 결과로 항상 본문과 일치.
  - 무음샷(`has_subtitle_track && subtitle_lines.is_empty()`): `dub_lang` 무관 동일 무음 지시문 반환.
  - `translate_line_to_korean()` / `translate_lines_to_korean_m3()` 함수는 유지 (직접 호출 단위 테스트 호환).
  - `PromptGenContext::translate_dub: bool` 추가.
- `src/decompile/mod.rs`: `DecompileOptions::translate_dub` 추가. 기본 `false` → scene-bible 번역·per-shot M3/테이블 번역 모두 게이트.
  `translate-dub` 명시 시에만 레거시 KO 더빙 경로 동작.
- `src/cli.rs` / `src/main.rs` / `src/mcp.rs`: `decompile --translate-dub` 플래그 배선 (MCP `translate_dub` 인자 포함).

### 요구사항 B: 6개국어 외부 자막 + 번역 캐시 (`src/subtitles.rs` 신설)
- `SUPPORTED_LANGS = ko/zh/ja/en/fr/de`, `format_timestamp()` / `format_srt()` / `write_srt()`.
- 원본 소스: `<proj>/story/transcript.srt` (타임코드 원본 유지). 없을 때 빈 큐 → 6개 파일도 0큐 유효 SRT로 생성.
- 캐시: `<proj>/story/subtitles_cache/<lang>.srt`. 존재+`!force`면 API 없이 즉시 복사.
- 온라인(`MINIMAX_API_KEY` 존재 && `!offline`) → `translate_cues_to_lang_m3()` (30줄 청크, 재시도 3회, 개수 불일치 시 에러→폴백).
  오프라인/키없음/실패 → `fallback_translate_line()` (ko 원문, 타 언어 `[CODE] 원문` 태그) — 6개 파일 모두 유효 SRT·동일 큐 개수 보장.
- `generate_multilang_subtitles()`가 `<movie>_<profile>.<lang>.srt` 6종 + 항상 `.proof.srt` 생성.

### 요구사항 C: 기본 오디오 = 원본 비디오 오디오
- `assemble_final_movie()`: H3(a:0)+원본(a:1) 듀얼트랙 유지, 원본에 `-disposition:a:<idx> default` (단독 트랙이어도 default).
  ffprobe 결과: index 1 (H3) `default=0`, index 2 (Original) `default=1`.
- `build_single_shot_compare()`: 원본 클립 오디오 우선 매핑 (`orig_has_audio` → `0:a:0`), 기존 `H3 우선`에서 변경.

### 요구사항 D: 증명용 출력 (`--proof`)
- `assemble --proof` (+ `--offline`) 플래그, `AssembleOptions { proof, offline }`, MCP `assemble_movie`에 `proof`/`offline` 스키마 추가.
- `.proof.srt`는 항상 생성: `timeline.json` 캐릭터 매핑으로 `[화자] 본문` 1줄 통합 (기존 `[Sx]` 태그 중복 제거).
- `--proof` 시 `<movie>_<profile>_proof.mp4` 생성: `subtitles` 번인 시도 → libass 미지원 빌드에서는 최종본 복사로 폴백 (항상 성공).

## 3. 검증
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과 (84 passed).
- `regression-tests/verify-issue-11.sh` 신규: Check 1 (EN/KO 원문+태그+무음), Check 2 (6종+proof.srt 큐·타임코드+캐시),
  Check 3 (`--proof` mp4), Check 4 (ffprobe default disposition) 전부 PASS.
- 기존 `verify-issue-4/5/6/8/9/10.sh` 호환 유지 (Check 10 PASS 확인, 9/6 백그라운드 병행 검증).
- 커밋은 하지 않고 테스트 통과 상태까지만 완료 (지시 준수).

## 4. 사용법
- 원문 유지 (기본): `rea-movie decompile in.mp4 -o proj --offline`
- 레거시 KO 더빙: `rea-movie decompile in.mp4 -o proj --translate-dub [--offline 제외 + MINIMAX_API_KEY]`
- 자막+조립: `rea-movie assemble --proj-dir proj --smoke --offline`
- 증명 비디오: `rea-movie assemble --proj-dir proj --smoke --offline --proof`
