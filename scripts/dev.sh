#!/usr/bin/env bash
# 开发服务器：隔离数据目录 + Vite dev CORS
set -euo pipefail
cd "$(dirname "$0")/.."
export APP_DATA_DIR="${APP_DATA_DIR:-./data}"
mkdir -p "$APP_DATA_DIR/logs"
[[ -f "$APP_DATA_DIR/app.token" ]] && chmod 600 "$APP_DATA_DIR/app.token"
APP_HTTP__DEV_CORS="${APP_HTTP__DEV_CORS:-true}" exec cargo run --bin story-architect
