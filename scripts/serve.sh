#!/usr/bin/env bash
# 本地开发服务管理：start | stop | restart | status | log
#
# 为什么需要它：直接以「后台任务」方式跑 `cargo run` 会一直显示为运行中，
# 看起来像卡死。这里用 nohup + disown 把服务变成脱离终端的普通进程，
# 命令本身立即返回，日志固定写到 /tmp/story-architect.log。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${SF_BIN:-$ROOT/target/debug/story-architect}"
if [[ ! -x "$BIN" ]]; then
  BIN="/Users/alex/.cargo/global-target/debug/story-architect"
fi
LOG="${SF_LOG:-/tmp/story-architect.log}"
PORT="${APP_PORT:-8080}"

pid_of() {
  # lsof 在没有匹配时返回 1，配合 pipefail 会让 set -e 直接退出，这里显式吞掉。
  { lsof -nP -iTCP:"$PORT" -sTCP:LISTEN -t 2>/dev/null || true; } | head -1
}

health() {
  curl -s -m 3 "http://127.0.0.1:$PORT/api/health" 2>/dev/null || true
}

start() {
  local pid
  pid="$(pid_of)"
  if [[ -n "$pid" ]]; then
    echo "已在运行 pid=$pid"
    health
    return 0
  fi
  if [[ ! -x "$BIN" ]]; then
    echo "找不到可执行文件：$BIN（先跑 scripts/check.sh 或 cargo build）" >&2
    return 1
  fi
  : >"$LOG"
  nohup env APP_NO_OPEN=1 "$BIN" >>"$LOG" 2>&1 &
  disown || true
  for _ in $(seq 1 40); do
    if [[ -n "$(pid_of)" ]] && [[ -n "$(health)" ]]; then
      echo "已启动 pid=$(pid_of) 日志=$LOG"
      health
      return 0
    fi
    sleep 0.25
  done
  echo "启动失败，最后 20 行日志：" >&2
  tail -20 "$LOG" >&2
  return 1
}

stop() {
  local pid
  pid="$(pid_of)"
  if [[ -z "$pid" ]]; then
    echo "未在运行"
    return 0
  fi
  kill "$pid"
  for _ in $(seq 1 40); do
    [[ -z "$(pid_of)" ]] && { echo "已停止 pid=$pid"; return 0; }
    sleep 0.25
  done
  echo "优雅退出超时，强制结束 pid=$pid" >&2
  kill -9 "$pid" 2>/dev/null || true
}

status() {
  local pid
  pid="$(pid_of)"
  if [[ -z "$pid" ]]; then
    echo "未在运行（端口 ${PORT}）"
    return 3
  fi
  echo "运行中 pid=$pid 端口=$PORT 日志=$LOG"
  health
}

case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status) status ;;
  log) tail -n "${2:-50}" "$LOG" ;;
  *) echo "用法: $0 {start|stop|restart|status|log [n]}" >&2; exit 2 ;;
esac
