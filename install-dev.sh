#!/bin/sh
# install-dev.sh — rea-movie developer installer (local build, no download).
# Builds the binary and links it + the skill into the local environment so
# `rea-movie` works immediately after every build.
#
# Usage:
#   ./install-dev.sh [--debug] [--copy] [--no-build] [--skill-only]
#
#   (no args)    cargo build --release, symlink target/release/rea-movie -> ~/.local/bin
#   --debug      cargo build (debug), symlink target/debug/rea-movie instead (fast iteration)
#   --copy       copy the binary instead of symlinking (for isolated testing)
#   --no-build   skip cargo build, only (re)link existing binary + skill
#   --skill-only only (re)install the Claude skill, skip build + binary link
#
# Result:
#   ~/.local/bin/rea-movie  (symlink or copy of this repo's build)
#   ~/.claude/skills/rea-movie/ (copy of ./skills/rea-movie/)
set -eu

BIN_NAME="rea-movie"
REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
SKILL_SRC="${REPO_ROOT}/skills/rea-movie"
SKILL_DIR="${HOME}/.claude/skills/rea-movie"

MODE="release"
COPY="0"
BUILD="1"
SKILL_ONLY="0"

for arg in "$@"; do
  case "${arg}" in
    --debug) MODE="debug" ;;
    --copy) COPY="1" ;;
    --no-build) BUILD="0" ;;
    --skill-only) SKILL_ONLY="1" ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *) echo "[install-dev] unknown option: ${arg} (see --help)" >&2; exit 1 ;;
  esac
done

if [ "${SKILL_ONLY}" = "0" ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "[install-dev] ERROR: cargo not found. Install Rust toolchain first:" >&2
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
    exit 1
  fi
  if [ "${BUILD}" = "1" ]; then
    echo "[install-dev] building (${MODE}) in ${REPO_ROOT}..."
    if [ "${MODE}" = "debug" ]; then
      (cd "${REPO_ROOT}" && cargo build)
      BUILT_BIN="${REPO_ROOT}/target/debug/${BIN_NAME}"
    else
      (cd "${REPO_ROOT}" && cargo build --release)
      BUILT_BIN="${REPO_ROOT}/target/release/${BIN_NAME}"
    fi
  else
    if [ "${MODE}" = "debug" ]; then
      BUILT_BIN="${REPO_ROOT}/target/debug/${BIN_NAME}"
    else
      BUILT_BIN="${REPO_ROOT}/target/release/${BIN_NAME}"
    fi
  fi
  if [ ! -x "${BUILT_BIN}" ]; then
    echo "[install-dev] ERROR: binary not found: ${BUILT_BIN}" >&2
    echo "  run ./install-dev.sh (without --no-build) first." >&2
    exit 1
  fi
  mkdir -p "${BIN_DIR}"
  if [ "${COPY}" = "1" ]; then
    echo "[install-dev] copying ${BUILT_BIN} -> ${BIN_DIR}/${BIN_NAME}"
    cp -f "${BUILT_BIN}" "${BIN_DIR}/${BIN_NAME}"
    chmod +x "${BIN_DIR}/${BIN_NAME}"
  else
    echo "[install-dev] linking ${BIN_DIR}/${BIN_NAME} -> ${BUILT_BIN}"
    ln -sfn "${BUILT_BIN}" "${BIN_DIR}/${BIN_NAME}"
  fi
fi

if [ ! -d "${SKILL_SRC}" ]; then
  echo "[install-dev] ERROR: skill source not found: ${SKILL_SRC}" >&2
  exit 1
fi
mkdir -p "${SKILL_DIR}"
echo "[install-dev] syncing skill ${SKILL_SRC}/ -> ${SKILL_DIR}/"
cp -rf "${SKILL_SRC}/." "${SKILL_DIR}/"

echo "[install-dev] installed: ${BIN_DIR}/${BIN_NAME} + ${SKILL_DIR}/"
case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *) echo "[install-dev] NOTE: ${BIN_DIR} is not on PATH. Add:" >&2
     echo "  export PATH=\"\$HOME/.local/bin:\$PATH\"" >&2 ;;
esac

"${BIN_DIR}/${BIN_NAME}" --version
if ! "${BIN_DIR}/${BIN_NAME}" pool status; then
  echo "[install-dev] NOTE: 'pool status' failed — check [pool] nodes / SSH access (see README.md)." >&2
fi
echo "[install-dev] ok — try: ${BIN_NAME} --help"
