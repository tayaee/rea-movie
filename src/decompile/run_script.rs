use anyhow::{Context, Result};
use std::path::Path;

pub fn generate_run_script(proj_dir: &Path) -> Result<()> {
    let script_path = proj_dir.join("run.sh");

    let script_content = r#"#!/usr/bin/env bash
set -euo pipefail

# 1. Locate rea-movie binary
if [ -n "${REA_MOVIE_BIN:-}" ] && [ -x "$REA_MOVIE_BIN" ]; then
  BIN="$REA_MOVIE_BIN"
elif [ -x "$(command -v rea-movie 2>/dev/null)" ]; then
  BIN="$(command -v rea-movie)"
else
  PROJ_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  CUR="$PROJ_DIR"
  BIN=""
  while [ "$CUR" != "/" ]; do
    if [ -x "$CUR/target/release/rea-movie" ]; then
      BIN="$CUR/target/release/rea-movie"
      break
    elif [ -x "$CUR/target/debug/rea-movie" ]; then
      BIN="$CUR/target/debug/rea-movie"
      break
    fi
    CUR="$(dirname "$CUR")"
  done
  if [ -z "$BIN" ]; then
    echo "Error: rea-movie binary not found. Please build rea-movie or set REA_MOVIE_BIN." >&2
    exit 1
  fi
fi

# 2. Parse arguments
SHOT_ARG=""
SMOKE_FLAG=""
FORCE_FLAG=""
FORCE_VIDEO_ONLY_FLAG=""
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --smoke)
      SMOKE_FLAG="--smoke"
      shift
      ;;
    --force)
      FORCE_FLAG="--force"
      shift
      ;;
    --force-video-only)
      FORCE_VIDEO_ONLY_FLAG="--force-video-only"
      shift
      ;;
    *)
      if [[ "$1" =~ ^[0-9]+(-[0-9]+)?$ ]]; then
        SHOT_ARG="--shot $1"
      else
        EXTRA_ARGS+=("$1")
      fi
      shift
      ;;
  esac
done

PROJ_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJ_ROOT"

# 3. Execute render then assemble
echo "==> Running render step..."
"$BIN" render $SHOT_ARG $SMOKE_FLAG $FORCE_FLAG $FORCE_VIDEO_ONLY_FLAG ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}

echo "==> Running assemble step..."
"$BIN" assemble $SMOKE_FLAG $FORCE_FLAG

echo "==> Run completed successfully."
"#;

    std::fs::write(&script_path, script_content)
        .with_context(|| format!("Failed to write run.sh to {:?}", script_path))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms)
            .with_context(|| format!("Failed to set executable permission on {:?}", script_path))?;
    }

    Ok(())
}
