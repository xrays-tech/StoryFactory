#!/usr/bin/env bash
# 统一质量门：格式化 → 静态检查 → 测试。
#
# 用法：
#   scripts/check.sh                     # 快速门（fmt + clippy + 定向测试，约 1-2 分钟）
#   scripts/check.sh --full              # 含全量测试（约 2-5 分钟，视增量编译而定）
#   scripts/check.sh --test novel_import # 只跑指定集成测试（可重复）
#
# 设计要点：
# - 全部输出直接流式打印，不重定向到文件、不经过任何包装器（避免"静默跑很久"）。
# - 每个阶段打印耗时，慢在哪一眼可见。
# - 不并发调用 cargo：所有项目共享 CARGO_TARGET_DIR，并发只会互相等锁。
set -euo pipefail

cd "$(dirname "$0")/.."

# 直接用工具链里的 cargo，避开 shell 包装器（rtk 等）对输出的缓冲/污染。
if [[ -n "${CARGO_BIN:-}" ]]; then
  CARGO="$CARGO_BIN"
elif [[ -x "${HOME}/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/cargo" ]]; then
  CARGO="${HOME}/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/cargo"
else
  CARGO="$(command -v cargo)"
fi

MODE="fast"
TESTS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --full) MODE="full"; shift ;;
    --test) TESTS+=("$2"); shift 2 ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "未知参数：$1（用 --help 查看用法）" >&2; exit 2 ;;
  esac
done

TOTAL=2
if [[ ${#TESTS[@]} -gt 0 ]]; then
  TOTAL=$(( TOTAL + ${#TESTS[@]} ))
elif [[ "$MODE" == "full" ]]; then
  TOTAL=$(( TOTAL + 1 ))
else
  TOTAL=$(( TOTAL + 4 ))
fi
STEP=0
FAILED=()

run_phase() {
  local name="$1"; shift
  STEP=$(( STEP + 1 ))
  local start
  start=$(date +%s)
  printf '\n\033[1m[%d/%d] %s\033[0m\n' "$STEP" "$TOTAL" "$name"
  if "$@"; then
    printf '\033[32m  ✓ %s (%ss)\033[0m\n' "$name" $(( $(date +%s) - start ))
  else
    local code=$?
    printf '\033[31m  ✗ %s (%ss)\033[0m\n' "$name" $(( $(date +%s) - start ))
    FAILED+=("$name")
    return "$code"
  fi
}

echo "cargo: $CARGO"
echo "模式: $MODE $([[ ${#TESTS[@]} -gt 0 ]] && echo "定向: ${TESTS[*]}")"

run_phase "cargo fmt --check" "$CARGO" fmt --all -- --check || true
run_phase "cargo clippy" "$CARGO" clippy --all-targets -- -D warnings || true

if [[ ${#TESTS[@]} -gt 0 ]]; then
  for t in "${TESTS[@]}"; do
    run_phase "cargo test --test $t" "$CARGO" test --test "$t" || true
  done
elif [[ "$MODE" == "full" ]]; then
  run_phase "cargo test（全量）" "$CARGO" test || true
else
  # 快速门只跑改动概率最高的几组；提交前用 --full。
  for t in health p2 sandbox novel_import; do
    STEP=$(( STEP + 1 ))
    run_phase "cargo test --test $t" "$CARGO" test --test "$t" || true
  done
fi

printf '\n'
if [[ ${#FAILED[@]} -gt 0 ]]; then
  printf '\033[31m失败：%s\033[0m\n' "${FAILED[*]}"
  exit 1
fi
printf '\033[32m全部通过\033[0m\n'
