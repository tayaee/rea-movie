# Issue 6: 조립 시 샷 길이 정합 — 렌더 클립을 원본 샷 길이로 트리밍

## 1. 배경 및 문제

`src/render.rs:607`의 `let h3_duration = shot.duration.clamp(4.0, 15.0);` 때문에
**4초보다 짧은 샷은 H3에 4초로 요청된다.** 실측: 원본 1.17초 샷이 4초로,
4.0초 샷이 4.48초로 렌더되었다.

결과:
- 조립된 영화가 원본(120초)보다 길어지고, 원본 타임라인과 싱크가 어긋난다.
- `_compare.mp4`는 `hstack`(원본 클립 | 렌더 클립)이라 길이가 다른 두 스트림을
  framesync로 합치며, 짧은 쪽의 마지막 프레임을 반복(freeze)한다.

## 2. 목표

조립(`rea-movie assemble`) 시 각 샷의 렌더 영상을 **원본 샷 길이(timeline duration)**로
정확히 트리밍한 뒤 concat 한다. 본편과 비교 영상 모두 동일한 트리밍 결과를 사용한다.

## 3. 상세 요구사항

1. `collect_assemble_shots`가 `timeline.json`에서 각 샷의 `duration`을 함께 읽어
   `AssembleShot`에 `duration: Option<f64>` 필드로 보관한다(timeline이 없으면 None).
2. `assemble_final_movie`와 `assemble_compare_movie`에서 렌더 영상을 사용할 때:
   - 해당 샷의 `duration`이 있고 렌더 클립의 실제 길이가 `duration + 0.08`초보다 크면
     **트림된 임시 클립**을 만든다.
     - 임시 위치: 출력 디렉터리 아래 `.trim/<shot_id>.mp4` (조립 끝에서 삭제).
     - 인코딩: `-t <duration>` + 정확히 끊기 위해 재인코딩
       (`-c:v libx264 -preset veryfast -crf 18 -c:a aac -ar 32000 -ac 2`).
   - 렌더 길이가 duration 이내이면 원본 파일을 그대로 쓴다(불필요한 재인코딩 방지).
   - 비교 영상의 좌측(원본 클립)도 같은 길이로 트리밍해 좌우 싱크를 맞춘다.
3. 트리밍은 원본 클립이 존재하지 않는 경우(렌더만 있는 경우)에도 동일하게 적용한다.
4. 기존 동작(트림이 필요 없는 경우)은 **바이트 단위로 동일**해야 한다.

## 4. 검수 기준

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
2. `regression-tests/verify-issue-6.sh` 통과:
   - 짧은 샷(1초대)의 렌더가 원본 샷 길이로 트리밍되어 조립본 총 길이가
     원본 총 길이 ±0.5초 이내로 수렴한다.
   - 트리밍이 필요 없는 경우 원본 파일이 그대로 사용된다(불필요한 재인코딩 없음).
   - 임시 파일이 조립 후 남지 않는다.
3. `breakingbad-s5e7-v2` 프로젝트에서 조립 결과 총 길이가 원본 120.02초 ±0.5초.

## 5. 구현 결과

1. **`src/assemble.rs` 구현 내용**:
   - `AssembleShot` 구조체에 `pub duration: Option<f64>` 필드 추가.
   - `collect_assemble_shots`: `timeline.json`이 존재할 때 각 샷의 `duration`을 매핑하여 보관(`timeline.json`이 없을 경우 `None`).
   - `probe_video_duration`: `ffprobe` format 및 stream 정보를 통해 비디오 클립의 실제 길이를 정확히 측정.
   - `probe_video_fps`: 비디오 스트림의 `r_frame_rate`를 감지하여 프레임 레이트 파싱.
   - `trim_video_clip`: 프레임 레이트 기반으로 타겟 duration을 정렬(`(duration * fps).round() / fps`)한 뒤, `-t <trim_dur>` 및 재인코딩(`-c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -c:a aac -ar 32000 -ac 2 -f mp4`) 옵션으로 임시 클립 생성.
   - `prepare_effective_shots`: 렌더 클립 및 원본 클립의 실제 길이가 `target_dur + 0.08`초를 초과할 경우 `.trim/<shot_id>.mp4` 및 `.trim/<shot_id>_orig.mp4`로 트림하고, 기존에 생성된 트림본이 있으면 재사용하여 불필요한 재인코딩 방지.
   - `run_assemble`: 조립 완료 후(또는 실패 시) 임시 디렉터리 `.trim/` 완전 정리.
   - `assemble_final_movie` 및 `assemble_compare_movie`: 공통적으로 `prepare_effective_shots`를 사용하여 본편과 좌우 비교 영상 간 동일한 트리밍 결과와 완벽한 좌우/오디오 싱크 보장.
2. **단위 테스트 추가** (`src/assemble.rs` 내 3개 신규 테스트):
   - `test_probe_video_duration_and_trim_video_clip`: 비디오 길이 측정 및 트림 정상 동작 확인.
   - `test_prepare_effective_shots_trim_threshold_and_reuse`: 트리밍 임계값(0.08초), 원본 유지, 원본 클립 누락 시 렌더 트림, 기생성 트림본 재사용 확인.
   - `test_collect_assemble_shots_reads_timeline_duration`: `timeline.json` 유무에 따른 `AssembleShot.duration` 적재 검증.
3. **회귀 검증 테스트** (`regression-tests/verify-issue-6.sh`):
   - Check 1: 1초대 짧은 샷들이 타임라인 길이로 트리밍되어 총 길이 5.5s에 ±0.5s 이내(5.59s)로 수렴.
   - Check 2: duration + 0.08s 이내인 샷은 트리밍 없이 원본 파일 그대로 사용하여 불필요한 재인코딩 방지.
   - Check 3: 조립 완료 후 임시 디렉터리(`.trim/`) 자동 삭제 확인.
   - Check 4: 원본 클립이 없는 렌더 전용 프로젝트에서도 정상 트리밍 및 조립 확인.
   - Check 5: `breakingbad-s5e7-v2` 프로젝트 실제 조립 결과: 본편 120.108초, 비교 120.099초로 원본 120.02초 대비 오차 0.08초 수준으로 완벽 수렴.
4. **품질 검사 통과**:
   - `cargo fmt --check`: 통과
   - `cargo clippy --all-targets -- -D warnings`: 경고 0건 통과
   - `cargo test`: 전체 37개 단위 테스트 통과
   - `regression-tests/verify-issue-6.sh`: 전체 5개 체크 패스 통과
