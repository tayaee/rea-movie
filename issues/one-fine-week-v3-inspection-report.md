# one-fine-week-v3 역공학 및 원본/v2 대비 긴 대사 장면 연결성 정밀 검사 보고서

날짜: 2026-11-18 (EST) / 범위: `EP01.mp4 --range 0-120` → `h3-projects/one-fine-week-v3` (+`h3-results/one-fine-week-v3`)
방법: `series-watch` → decompile → v2 시드 + 프롬프트 정제 → `run.sh --smoke` → 실서버 H3 단발 검증 → `assemble --smoke --proof --force` → Python/ffprobe/PIL 정량 비교

## 0. provenance (선행 고지 — 수치 해석에 필수)

1. **v4 세션과 경합**: 작업 중 동일 머신의 별도 세션이 `one-fine-week-ep01/ep02/ep03-v4` + `one-fine-week-v4-inspection-report.md`를 선행 완성했다.
   내 `one-fine-week-v3` 초기 decompile 산출물은 해당 세션에 의해 삭제되었고, 내 decompile 프로세스도 고아가 되어 중단시켰다(API 과금 방지).
   이에 v3는 **ep01-v4(66샷/11씬, v2 M3 비전 베이스 계승)를 복제**하여 출발했고, 아래 §2의 정제 작업을 v3만의 실질 개선으로 수행했다.
2. **v2 산출물 삭제**: `h3-results/one-fine-week-ep01-v2`(조립본)와 `h3-projects/one-fine-week-ep01-v2`(프롬프트) 모두 경합 세션에 의해 삭제됐다.
   v2 수치는 (a) 내가 decompile 대기 중 직접 읽은 v2 프롬프트 기록과 (b) v4 리포트의 v2 정량표(동일 방법론 `cum+dur-0.05`) 인용으로 대체한다. 출처를 각 표에 명기.
3. **바이너리 변경**: v4 조립본(06:05경, 구 바이너리)과 v3 조립본(22:38, 신 바이너리) 사이에
   `ede0abb feat(issue-7)`이 `src/assemble.rs`의 `retime_render_clip`을 변경했다(아래 §4b).
   즉 **끝-프레임 수렴도는 어셈블러 버전의 함수**이며, v3 프롬프트 품질과 독립 변수다.

## 1. series-watch 및 sources/ 캐시

- `./target/debug/rea-movie series-watch --source-dir ~/rea-movie-artifacts/source/one-fine-week --out-root ~/rea-movie-artifacts` 실행, `EXIT:0`.
- 결과(04:21, mode=**m3**): `story/world.json`(13.8KB) · `story/characters.json`(15.9KB) · `story/timeline-days.md`(5.6KB),
  `sources/`에 EP01~EP03 probe/keyframe 캐시 확보(115/152/176 키프레임, 재사용).
- 이후 10편 `--force` 실행(별도 세션, 04:36~04:48)이 `story/`를 10편 오프라인 초안으로 덮어썼고,
  v4 세션이 이를 다시 수동 보강한 상태가 현재값이다. 본 과업의 선행 조건(파일 존재)은 충족 유지.

## 2. v3 역공학 (decompile)

- `./target/debug/rea-movie decompile EP01.mp4 --range 0-120 -o h3-projects/one-fine-week-v3 --force` 실행.
  66샷 검출(63 cuts, max 8.0s), 330 에셋 추출, **11씬 그룹핑**까지 진행됐으나 장시간 M3 호출 중 백그라운드 프로세스가 2회 SIGTERM으로 소실됐다.
- 복구: ep01-v4 복제 → v3 정제 스크립트(`/tmp/opencode/clean-v3.py`)로 아래 보강을 수행했다(전량 카운트 검증):
  - **워터마크/로고 묘사 문장 삭제**(12곳): `broadcast watermark`, `broadcaster watermark/logo`, `platform watermark`,
    `translucent watermark`, `Korean broadcaster watermark`, `방송 채널 표시` — ko/h3 전수, 잔존 0건.
  - **자막·타이틀 오버레이 묘사 삭제**(shot_0007~0019, 18곳): `시라진 타이틀 오버레이`, `자막이 나타난다/겹쳐 보인다/배치된다`류 — 잔존 0건(해당 구간).
    ※ 초점 구간 밖(~20곳)은 스코프 외로 남김 — 한계 §5.
  - **대사 glitch 수정**: `자ught어` → `자랐어` (shot_0012 h3, 4건).
  - **ko 앵커 라인 추가**(13샷): `<!-- rea-movie:anchor:begin -->` + `<Picture 1>(첫 프레임)→<Picture 2>(끝 프레임)` 델타·180도축 유지 명시.
    h3에는 기존 `<Picture 1> and <Picture 2> (from [Shot N]) are fully referenced.` + `Anchor transition ... preserving the 180-degree scene axis` 유지 확인.
- 씬 바이블 `story/scenes/scene_*.json` 11개 존재. `scene_0001`(0.0~22.6s, 12샷)에 spatial_map/eyeline_notes/axis_of_action/camera_grammar/dialogue_flow 포함 확인.
  - 기지 한계: 오프라인 바이블의 인물 ID 서술(S1=애시블론드 정다은 등)이 M3 비전 베이스(S1=다크브라운 포니테일 카페 직원)와 불일치.
    비전 재호출 없이 확정 불가 → 수정하지 않고 한계로 기록(§5).

## 3. H3 렌더, run.sh, assemble

- 시딩: v2 smoke 66클립 → v4를 거쳐 v3 `shots/shot_*/renders/smoke/`에 66개 확보(검증 `find … | wc -l` = 66).
- `REA_MOVIE_BIN=... ./run.sh --smoke` → **EXIT:0**. render 스킵(66/66 seeded) + assemble로
  `EP01_smoke.mp4`(10.2MB/122.18s) + `EP01_compare.mp4`(14.9MB) + 7 srt sidecar 생성.
- **실서버 H3 검증**: `./run.sh 0008 --smoke --force` → spark1에 제출, `prompt.ko.md → prompt.h3.txt` 컴파일 정상,
  그러나 300s 내 미완료(**EXIT:124, 타임아웃**) → 과업 지시대로 시딩 본 유지(기존 render 보존, `history/take_001.mp4` 백업만 생성).
  동기화 컴파일이 h3를 재생성한 점은 파이프라인 정상 동작으로 확인(씬 컨텍스트·Picture refs 유지).
- `/target/debug/rea-movie assemble --proj-dir …/one-fine-week-v3 --smoke --proof --force` 실행 → **EXIT:0** (23:24 완료):
  - `EP01_smoke.mp4`(10.2MB/122.18s), `EP01_compare.mp4`(14.9MB/121.03s, 좌원본·우v3) 완료.
  - 6개국어 M3 번역 순차 완료(ko 22:53 → zh 23:03 → en·ja 23:14 → fr 23:19 → de 23:24, 실제 번역문 확인 — 초기 폴백 `[XX]` 접두본은 전부 교체됨.
    fr 1차 시도 `Translation count mismatch (29/30)` 경고 후 재시도로 성공).
  - `EP01_smoke_proof.mp4`(7.8MB/122.17s, 화자 태그 번인) + `.proof.srt`(화자 태그 `[S1]` 등) + 6개국 srt = **10파일 완비**.
- 오디오: `a:0 H3 default=0`, **`a:1 Original default=1`** (ffprobe 확인) — 원본 오디오 default 요구 충족.

## 4. 원본 vs v2 vs v3 정밀 비교 (shot_0007~shot_0019, 10.71s~45.18s)

### a) 씬 그룹핑

| scene | 샷 범위 | 구간 | 비고 |
|---|---|---|---|
| scene_0001 | 0001→0012 | 0.00~22.56s | 초점 전반(0007~0012) 포함. A-B-A 교차 6앵글 단일씬 유지 ✓ |
| scene_0002 | 0013→0018 | 22.56~42.31s | 초점 후반(0013~0018) |
| scene_0003 | 0019→0020 | 42.31~45.18s~ | 0019부터 |

- v2에는 씬 개념 자체가 없음(`story/scenes/` 부재) → v3(11씬, A-B-A 2/3-hop lookback)가 구조적으로 우위.
- 단, 35초 카페 대화가 **3개 씬으로 분할**된다. 경계 연속성(회색조 32bin 히스토그램 상관):
  - 0012→0013(분할점): **0.785** — 씬 내 컷 0007→0008(0.737)보다 높고, 씬 내 하드앵글 0011→0012(0.326)보다 훨씬 높음 → **22.56s 분할은 과분할(over-segmentation) 소지**.
  - 0018→0019(분할점): **0.339** — 강한 불연속(장소/구도 점프) → 분할 정당.
- 결론: A-B-A lookback은 0007~0012(6연속 교차편집)를 한 씬에 묶는 데 성공했으나, 동일 대화 Erin 0013 이후를 묶지 못했다.

### b) 컷 전환 경계 연결성 (조립본 끝-프레임 vs 원본 `last_frame.jpg`)

방법: 각 샷 누적 종료점 `cum+dur-0.05s` 프레임 추출 → 256×144 회색조 32bin 히스토그램 상관 / L1 / PSNR. v3·v4는 본 리포트 스크립트(`/tmp/opencode/quant-v3.py`)로 fresh 측정,
v2는 v4 리포트표 인용(v2 조립본 삭제됨 — §0-2).

| shot | dur | v2_hist* | v3_hist | v4_hist | v2_L1* | v3_L1 | v4_L1 | v2_PSNR* | v3_PSNR | v4_PSNR |
|---|---|---|---|---|---|---|---|---|---|---|
| 0007 | 3.57 | 0.973 | 0.959 | 0.955 | 8.8 | 8.6 | 4.3 | 23.0 | 23.2 | 30.7 |
| 0008 | 1.57 | 0.952 | 0.940 | 0.944 | 5.0 | 5.2 | 4.5 | 28.8 | 28.9 | 31.8 |
| 0009 | 1.80 | 0.971 | 0.928 | 0.931 | 9.7 | 9.2 | 3.6 | 21.8 | 22.3 | 33.9 |
| 0010 | 1.13 | 0.993 | 0.984 | 0.988 | 10.0 | 9.9 | 3.6 | 21.2 | 21.3 | 33.5 |
| 0011 | 1.17 | 0.954 | 0.916 | 0.916 | 3.3 | 3.3 | 3.3 | 33.3 | 34.8 | 34.7 |
| 0012 | 2.60 | 0.991 | 0.984 | 0.985 | 8.0 | 5.7 | 3.8 | 22.9 | 27.1 | 33.3 |
| 0013 | 1.23 | 0.969 | 0.945 | 0.941 | 8.6 | 4.9 | 3.7 | 22.9 | 29.1 | 33.1 |
| 0014 | 1.74 | 0.995 | 0.990 | 0.990 | 8.8 | 11.0 | 3.7 | 21.2 | 19.8 | 32.1 |
| 0015 | 4.07 | 0.979 | 0.967 | 0.965 | 5.9 | 6.1 | 3.6 | 25.8 | 25.4 | 33.4 |
| 0016 | 4.37 | 0.998 | 0.987 | 0.990 | 3.7 | 7.7 | 3.0 | 31.2 | 23.4 | 35.3 |
| 0017 | 5.37 | 0.993 | 0.983 | 0.987 | 8.7 | 12.2 | 3.7 | 22.0 | 19.3 | 33.4 |
| 0018 | 2.97 | 0.989 | 0.967 | 0.966 | 8.9 | 5.8 | 3.3 | 21.8 | 26.8 | 34.5 |
| 0019 | 2.87 | 0.985 | 0.988 | 0.989 | 7.4 | 7.1 | 2.9 | 23.9 | 24.2 | 34.1 |
| **평균** | – | **0.980** | **0.964** | **0.965** | **7.45** | **7.05** | **3.62** | **24.6** | **25.0** | **33.4** |

`*` v2열은 v4 리포트 측정값 인용. v4열은 본 스크립트 fresh 재측정(리포트값과 ±0.4 내 일치).

**해석 (중요)**: v3 ≈ v2 수준이며 v4(구 바이너리)보다 크게 낮다. 원인은 프롬프트가 아니라 **어셈블러 변경**이다:
구 바이너리(Issue-9)는 `setpts`+`atempo` 전구간 리타임으로 끝-프레임(`last_frame`)에 정확히 도달한 반면,
신 바이너리(`ede0abb`, Issue-7 speech guard)는 발화 클립을 **1.0x 윈도우 트림**(최대 에너지 구간 `-ss/-t`, `detect_speech_window`)으로 잘라
끝-프레임이 렌더 중간에 멈춘다. 즉 과업 3a의 전제("리타임으로 끝 프레임 보존")는 현행 파이프라인에서 **발화 샷에 한해 성립하지 않는다**
(무음/합성 클립만 legacy `setpts` 경로). v4 리포트의 EP01/EP02 `--force` 재조립 `ffmpeg 1.0x trim` 실패 기록과도 정합한다.
음성 1.0x 보존(칩멍크 방지)과 앵커 수렴의 트레이드오프이며, **회귀가 아닌 의도된 설계 변경**으로 판단 — 단 과업 문구와 충돌하므로 본 항으로 명시한다.

### c) 연출서 품질 (v2 vs v3, 초점 구간)

| 항목 | v2 (직접 확인 + v4 리포트) | v3 (본 작업 후) |
|---|---|---|
| 씬 컨텍스트 | 없음 | `scene:begin` 13/13 (장소·시간·인물·공간배치·시선·180도축·카메라문법·대사흐름·조명) |
| `<Picture 1>/<Picture 2>` | 없음 | h3 13/13 헤더 + Anchor transition, ko 13/13 anchor 블록 |
| 원문 대사 | verbatim이나 `자ught어` 오자(h3 0012) | `자랐어` 수정, `<d>[Korean]` 유지 |
| On/Off-screen | 리액션 샷에 발화 서술 혼재 | 씬 `dialogue_flow`에 off-screen VO 명시(0007 S1 VO 등). per-shot `no lip movement` 주입은 미완(한계) |
| 로고/오버레이 | VA 로고·워터마크·자막·타이틀을 장면 요소로 서술(0007 '시라진'·자막, 0012 워터마크+자막, 0017 워터마크 등) | 초점구간 30건 삭제, 잔존 0건. 구간 밖 워터마크 12건도 전수 삭제 |

## 5. 한계 및 후속

1. ~~proof 번역(M3, 600s/콜) 대기 중~~ → **해결**: 23:24 전량 완료(EXIT:0). `EP01_smoke_proof.mp4` + 7 srt 확정(§3).
2. H3 실서버 렌더(shot_0008) 300s 타임아웃 → 시딩 유지. 새 프롬프트(오염 제거본)의 실서버 발화 품질은 미검증.
3. 초점 구간 밖 자막-as-content ~20건 잔존(스코프 외).
4. 씬 바이블 인물 ID vs M3 비전 베이스 불일치(scene_0001 S1/S2) — 비전 재호출 없이 불가, 미수정.
5. 22.56s 씬 분할(0012→0013, 유사도 0.785)은 그룹핑閾값 조정 후보.
6. 현행 바이너리의 발화샷 1.0x 트림은 과업 3a 문구와 충돌 — Issue-7 의도(음성 우선) vs 앵커 수렴 중 정책 결정 필요.

## 산출물 (전량 검증 완료)

- 프로젝트: `h3-projects/one-fine-week-v3` (66샷, 11씬, run.sh — `run.sh --smoke` EXIT:0)
- 결과: `h3-results/one-fine-week-v3/shots/*/renders/smoke/` (66) · `outputs/smoke/` 10파일:
  `EP01_smoke.mp4` · `EP01_compare.mp4` · `EP01_smoke_proof.mp4` · `.ko/.zh/.ja/.en/.fr/.de.srt` · `.proof.srt`
- 스크립트: `/tmp/opencode/clean-v3.py`, `/tmp/opencode/quant-v3.py`
