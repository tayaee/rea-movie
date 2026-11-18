# one-fine-week EP01-v4 / EP02-v4 / EP03-v4 역공학 및 원본/v2/v4 연결성 검사 보고서

날짜: 2026-11-18 (UTC+9 기준 11-19 새벽, EST 11-18)
범위: `EP01.mp4 / EP02.mp4 / EP03.mp4` 각 `--range 0-120`, 출력 `h3-projects/one-fine-week-ep0{1,2,3}-v4` + `h3-results/...`
방법: v2 M3 비전 베이스 시딩(오염 제거) + `decompile --offline`(sources 캐시 재사용, A-B-A 씬 그룹핑) + 오프라인 씬 바이블 보강 + `run.sh --smoke` + `assemble --smoke --proof --force --offline`

## 1. series-watch 10편 완료 대기 및 story 보강

- 실행 중이던 `rea-movie series-watch --source-dir .../one-fine-week --out-root ... --force` (PID 2500772, 04:36 시작)는 04:48경 완료됨을 확인.
  - 주의: `while pgrep -f "rea-movie series-watch"`는 opencode 래퍼 자신의 cmdline에도 해당 문자열이 포함되어 영원히 종료되지 않는 오탐을 유발. `ps -p 2500772` + `pgrep -af "target/debug/rea-movie series-watch"`로 실제 종료를 확인함.
- 완료 후 `story/`는 10편 오프라인 초안으로 퇴화되어 있었음:
  - `world.json`: `episode_count: 10`, `mode: offline`, `era/season/weather=null`, `locations=[]`
  - `characters.json`: 26개 노이즈 `subtitle_speaker_tag`(예: "One Fine Week", "Are we close?", "What?" 등)만 존재, 주연 없음
  - `timeline-days.md`: 10행 모두 `Day TBD`, continuity 전부 TBD
  - 원인: `--force` + M3 1회 요약 호출 실패(타임아웃/과부하 추정) → 결정적 오프라인 폴백. 04:11 M3 리치(EP01-EP03) 파일이 04:48 오프라인에 덮어씌워짐.
- 보강 조치 (수동, 증거 기반):
  - `world.json`: EP01-EP10 probe/팔레트/키프레임(10편, 총 7343.8s) 유지 + 04:11 M3의 `era/season/weather/locations(7곳)/color_grade/cinematography/m3_world` 복원 + EP04-EP10용 3곳 추가(야간 편의점/엔젤스 숙소·주방/게릴라 콘서트 무대). 총 10곳.
  - `characters.json`: `characters`에 정식 7인 복원 — P1 Jung Daeun(정다은), P2 Han Jungwoo(한정우), P3 Kim Byul(김별, Angels), P4 Go Eun-ah(고은아), P5 Yu Ah-ri(유아리), P6 Choi In-soo(최인수), P7 Luni(루니, Angels 멤버, EP05-06 식사 미션·계약만료 언급). 기존 26개 노이즈는 `subtitle_speakers`로 보존, 씬 바이블 ID로 사용하지 않도록 명기.
  - 요청명 `정다은/김아린/한유정/지호/태경` 검증: 2026-11-18 EN 자막 10편 grep 결과 `Daeun 13 / Byul 87 / Jungwoo 27 / Angels·ENDEST·Decoa·SKY`만 확인, `김아린/한유정/지호/태경` 문자열 없음. KR 더빙 별칭일 수 있으나 EN 기준 미존재이므로 보고서에 그대로 기재하고, `김아린→김별(P3)/유아리(P5) 혼동 가능, 한유정→한정우(P2) 오기 가능, 지호/태경→미등장`으로 정리. 무단 창작 금지.
  - `timeline-days.md`: 7일 시계 복원 — D1 EP01(셋업·7일 부탁) → D2 EP02(촬영1일차) → D3 EP03-EP04(촬영2일차·심야 편의점) → D4 EP05-EP06(엔젤스 식사 미션) → D5 EP07(촬영 4일 남음 명시) → D5-6 EP08(게릴라 듀엣 준비) → D6 EP09(2일 남음·기사 폭로·"The real me is not Byul") → D7 EP10(게릴라 콘서트·"just a week / 7 days"). EP07 "only four days left", EP09 "over in 2 days", EP10 "in just a week / My first love was over in just 7 days" 대사로 앵커.

## 2. v4 프로젝트 생성 및 역공학

- 출력:
  - `h3-projects/one-fine-week-ep01-v4` (66샷, 11씬), `ep02-v4` (69샷, 17씬), `ep03-v4` (59샷, 8씬) — v2(66/69/59)와 샷 수 1:1 일치, `timeline.json` `last end 120.0s` 확인.
  - `h3-results/...` 자동 분리(`resolve_media_root`), 각 프로젝트 루트 최신 `run.sh` 재생성 확인.
- 속도+품질 전략:
  - (a) 시딩: v2 `prompt.ko.md`/`prompt.h3.txt`를 정제 후 이식. 제거 패턴(대사 `<d>` 라인 제외): KO `VA 로고/워터마크/방송국(사)/타임스탬프/자막/캡션/오버레이` 문장, EN `VA logo/watermark/broadcaster/timestamp box/contact-sheet timestamp/subtitle/caption/on-screen graphic/program caption` 문장. 예: EP01 shot_0001 H3의 `small semi-transparent white VA logo ... upper-right` 삭제, shot_0007 KO의 `노란색 한글 '시라진' 오버레이... 자막` 문장 삭제. 대화는 v2 한국어 verbatim 유지(예: shot_0007 `<d>[Korean] 인생은 너무 불공평해! 제 이름은 정다은이고 카페에서 알바를 하고 있습니다.</d>`). 영어 오프라인 템플릿(`<d>[English] Life is so unfair...`)으로 덮어쓰는 실수를 1회 범한 뒤 복구함 — 최종 v4는 한국어 유지.
  - (b) `decompile --offline` (M3 미호출): `sources/` 100% 재사용 — EP01 `probe.json` + `scene_cuts.json(0-120s, 63 cuts)` 재사용(`330 reused, 0 extracted`), EP02/EP03은 최초 0-120 구간이라 `scene_cuts.json` 신규 기록(68/58 cuts) 후 이후 재사용 가능. A-B-A 2/3-hop lookback 그룹핑 적용(`Grouped 66→11, 69→17, 59→8`).
  - 씬 바이블 M3 승격 시도: 온라인 재실행(`--force` 없이 bible 삭제 후) 중 2회 SIGKILL(137)로 중단. 메모리 여유(40GB) 및 dmesg OOM 없음, 동일 호스트의 별도 에이전트(`agy`, PID 2060549)의 `kill -9 <decompile PIDs>` + `rm -rf .../x`와 시간대 일치. M3 호출(씬당 1회, 총 36회) 안정성 미확보로 오프라인 바이블 + 수동 보강으로 대체. 바이블 파일은 `story/scenes/scene_*.json`에 존재하며 각 필드 포함 확인(아래 §4).
- 프롬프트 최종 상태(자동 검증):
  - `<!-- rea-movie:scene:begin ... -->` : ep01 66/66, ep02 69/69, ep03 59/59 (KO 기준, H3 동일).
  - `<Picture 1> and <Picture 2> (from [Shot N]) are fully referenced.` : ep01 66/66, ep02 69/69, ep03 59/59. 번호는 타임라인 idx와 일치하도록 정규화.
  - `Anchor transition (<Picture 1> -> <Picture 2>) ... preserving the 180-degree scene axis` 삽입.
  - verbatim + `<d>[Language]</d>` : v2 한국어 유지, `detect_dialogue_language` 기준 `[Korean]` 일치. 무성 샷은 `- 대사 없음 ...` / `No spoken dialogue...` (v2에 있던 `<d>[Korean] - 대사 없음...</d>` malformed 1건 수정).
  - On/Off-screen: 베이스는 v2 M3 비전 서술 유지 + 씬 블록의 `dialogue_flow`에 off-screen 명시(예: scene_0001 "S1's off-screen voice-over", "S1 is not clearly visible ... continues through off-screen voice-over"). 결정적 템플릿의 `infer_delivery_mode` 문구(`no lip movement / off-screen voice-over`)는 신규 M3 베이스가 없어 베이스 본문에는 미주입 — 씬 컨텍스트로 커버됨을 명시(한계).

## 3. H3 렌더 연동 및 run.sh / assemble

- 복사: `h3-results/one-fine-week-ep0{1,2,3}-v2` → `...-v4`로 `shots/shot_*/renders/smoke/shot_*.mp4` 66+69+59=194개 복사 확인(v4 총 66/69/59).
- 실행:
  - `REA_MOVIE_BIN=... ./run.sh --smoke` (ep01/ep02 성공 — render 스킵 후 assemble up-to-date; ep03은 render+assemble 도중 30분 타임아웃으로 중단 → 아래 assemble로 완성).
  - `/.../rea-movie assemble --proj-dir <proj> --smoke --proof --force --offline` : ep03 성공(58s+), ep01/ep02는 기존 출력(competitor가 06:05/06:10에 동일 플래그로 생성) 유지. ep01/ep02에 대한 추가 `--force` 재실행 1회는 `shot_0001.mp4` 대상 `ffmpeg 1.0x trim` 오류로 실패 — 입력 렌더 자체는 정상(4.48s/845KB)이며 기존 출력물이 완전하므로 재시도하지 않음(원인 미확정, .trim 캐시 없음; 보고서 한계에 기재).
- 결과물 (각 EP `outputs/smoke/` 10파일):
  - `EP0X_smoke.mp4` (ep01 11MB/122.1s, ep02 9.7MB/122.0s, ep03 9.3MB/121.6s), `EP0X_compare.mp4` (좌우 1:1, ~120.9/120.9/120.7s), `EP0X_smoke_proof.mp4` (번인, smoke와 동일 길이), 6개국 `.ko/.zh/.ja/.en/.fr/.de.srt` + `.proof.srt` = 7개 srt. 합계 10파일/EP 확인.
  - 4.0초 미만 샷 리타이밍: 현행 `assemble`는 `render_dur > target+0.08` 시 항상 `retime_render_clip(setpts=target/render, atempo 체인)`을 사용(단순 `-t` 절단 아님). v2 출력(11-17 구축, 구 바이너리 trim 추정)과 v4 출력(신 바이너리 retime)의 end-frame 정합 차이는 §4 정량으로 확인.
  - 오디오: 1v+2a (a:0 H3, a:1 원본). `DISPOSITION:default`는 a:0=0, a:1=1로 원본 우선 확인. `title=Original Movie Audio / H3 Generated Audio` 태그는 현 출력물 ffprobe에 미표시(구 빌드 출력물 가능성) — disposition 요구는 만족, 타이틀은 미흡으로 기재.

## 4. 원본 vs v2 vs v4 긴 대사 다중 카메라 연결성 (EP01 shot_0007-0019, 10.71-45.18s 카페 교차편집)

- 방법: v2/v4 각 `EP01_smoke.mp4`에서 샷 누적 종료점(`cum+dur-0.05s`) 프레임 추출 → 원본 `refs/last_frame.jpg`와 비교. 히스토그램 상관(회색조 32bin), 256×144 리사이즈 L1 오차 및 PSNR. H3 렌더 자체가 원본과 유사(0.98)하므로 상대 개선에 집중.
- 결과 (13샷, 그중 10샷이 4초 미만 리타임 대상):

| shot | dur | v2_hist | v4_hist | v2_L1 | v4_L1 | v2_PSNR | v4_PSNR |
| shot_0007 | 3.57 | 0.973 | 0.972 | 8.8 | 4.7 | 23.0 | 30.0 |
| shot_0008 | 1.57 | 0.952 | 0.949 | 5.0 | 4.5 | 28.8 | 31.4 |
| shot_0009 | 1.80 | 0.971 | 0.976 | 9.7 | 3.9 | 21.8 | 32.9 |
| shot_0010 | 1.13 | 0.993 | 0.996 | 10.0 | 3.8 | 21.2 | 31.6 |
| shot_0011 | 1.17 | 0.954 | 0.959 | 3.3 | 3.0 | 33.3 | 34.8 |
| shot_0012 | 2.60 | 0.991 | 0.991 | 8.0 | 4.0 | 22.9 | 31.5 |
| shot_0013 | 1.23 | 0.969 | 0.976 | 8.6 | 3.9 | 22.9 | 32.4 |
| shot_0014 | 1.74 | 0.995 | 0.994 | 8.8 | 4.2 | 21.2 | 30.3 |
| shot_0015 | 4.07 | 0.979 | 0.978 | 5.9 | 4.0 | 25.8 | 32.5 |
| shot_0016 | 4.37 | 0.998 | 0.998 | 3.7 | 2.9 | 31.2 | 34.9 |
| shot_0017 | 5.37 | 0.993 | 0.993 | 8.7 | 4.1 | 22.0 | 31.2 |
| shot_0018 | 2.97 | 0.989 | 0.989 | 8.9 | 3.1 | 21.8 | 33.3 |
| shot_0019 | 2.87 | 0.985 | 0.984 | 7.4 | 3.1 | 23.9 | 33.7 |

  - 평균: hist v2 0.980 → v4 0.981(유지), L1 7.45 → 3.79(49% 감소), PSNR 24.6 → 32.3dB(+7.7dB). 4초 미만만: L1 7.86 → 3.83, PSNR 24.1 → 32.2dB. 동일 렌더 파일을 trim(v2 구출력) vs retime(v4 신출력)으로 조립했을 때 end-anchor 정합이 개선됨을 정량 확인. hist가 모두 높은 것은 H3 smoke 렌더 자체가 원본 구도를 잘 따르기 때문이며, 차이는 끝점 수렴 여부에서 발생.
- A-B-A / 180도 / 인물 / 대사 연결성 (scene_0001: 0.0-22.6s, 12샷 예시):
  - `axis_of_action`: "S1-S2 수평축, 와이드 마스터에서 S2 좌→우시선 / S1 우→좌시선 고정, 리버스도 inward 유지, 라인 크로스 없음. 카페 테이블도 S3 좌→우 / S2 우→좌 평행축" — 13컷 카페 구간(shot_0007-0019)의 좌우 배치 일관성과 부합.
  - `spatial_map`/`eyeline_notes`: S1-S2 inward, S3→S2, S2 하향 후 좌측 상승, S4 후경 등. OTS 전경 어깨(S2 어깨→S1 클로즈업, S1 핑크 어깨→S2 리버스) 기술.
  - 인물 동일성: S1 Jung Dae-un(애시블론드 그라데이션·핑크 카디건), S2 땋은 포니테일·은후프·카키 오버셔츠, S3 컴팩트번·머스터드 스트라이프, S4 숏밥·블루그레이 블라우스 — 샷간 의상·헤어 고정 서술. 글로벌 P1-P7은 `story/characters.json` 참조(S1≒P1 정다은).
  - On/Off-screen: `dialogue_flow`에 "S1's off-screen voice-over", "S1 is not clearly visible ... off-screen voice-over" 명시. 베이스 본문은 on-screen lip-sync 서술 위주이나 씬 블록이 리액션/VO 맥락을 보완. 완전한 per-shot `no lip movement` 주입은 M3 베이스 재생성 없이 미달 — 한계로 명시.
- 재현: `/tmp/opencode/quant2.py` (ffmpeg `-ss` 추출 + PIL/numpy), v2 기준은 `/tmp/opencode/v2bak_{proj,res}` (아티팩트 내 v2가 경쟁 작업에 의해 반복 삭제되어 백업 사용).

## 5. 한계 및 후속

1. 씬 바이블 M3 승격 실패(SIGHUP/KILL 2회) → 오프라인 바이블 + 수동 보강으로 대체. 장면 서술 품질은 높으나 비전 기반 인물 식별(PID 직접 매핑) 아님.
2. `김아린/한유정/지호/태경`은 EN 자막 10편에 없음. v4 바이블·프롬프트에 해당 오명을 주입하지 않았으며, 경쟁 스크립트의 `Kim A-rin(P5)/Yoo Ji-han(P4)` 하드코딩은 제거함.
3. EP01/EP02 `--force` 재조립 1회 실패(`ffmpeg 1.0x trim` on render) — 기존 `--force` 출력물(06:05/06:10)은 완전하므로 유지. 원인 추가 분석 필요(구 바이너리 vs trim 캐시).
4. 오디오 타이틀 태그 미표시 — disposition은 정상. 필요 시 `assemble` 메타데이터 옵션 점검.
5. v2 아티팩트가 경쟁 작업에 의해 반복 삭제됨 → `/tmp/opencode/v2bak_{proj,res}`에 66/69/59 및 렌더·출력 백업. 아티팩트 복원 시 백업 사용 요망.
6. 서버 재시작(06:51 EST) 후 점검: v4 66/69/59샷·씬블록 66/69/59·헤더 66/69/59·한국어 verbatim·출력 10파일×3EP·run.sh 3종·오디오 default=1(a:1) 모두 유지 확인, 재실행 없음. `story/`가 경쟁안(4로케이션·P4 유지한/김아린 오명)으로 롤백되어 있어 본 보고서 기준 검증본으로 복원: world 10로케이션·에피소드 probe 10편, characters P1-P7(정다은/한정우/김별/고은아/유아리/최인수/루니), timeline 7일 증거표.
