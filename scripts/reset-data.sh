#!/usr/bin/env bash
# 清空全部业务数据（小说/卷/章/角色/关系/道具/沙盒会话/导入任务/用量/幂等记录），
# 保留 app_config（LLM provider/base_url/model 等运行时配置）与 prompt_templates。
#
# 用法：
#   scripts/reset-data.sh            # 停服 -> 清空 -> 重启
#   scripts/reset-data.sh --no-start # 只清空，不重启
#   scripts/reset-data.sh --yes      # 跳过确认
#
# 注意：必须停服后执行。SQLite 只允许最后一个连接在退出时 checkpoint 并删除 -wal，
# 服务运行时用其它进程写入会让新旧连接指向不同的 WAL inode，导致写入丢失。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="${APP_DATA_DIR:-}"
if [[ -z "$DATA_DIR" ]]; then
  case "$(uname -s)" in
    Darwin) DATA_DIR="$HOME/Library/Application Support/StoryArchitect" ;;
    MINGW* | MSYS* | CYGWIN*) DATA_DIR="${APPDATA:-$HOME/AppData/Roaming}/StoryArchitect" ;;
    *) DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/story-architect" ;;
  esac
fi
DB="$DATA_DIR/story-architect.db"

START=1
ASSUME_YES=0
for arg in "$@"; do
  case "$arg" in
    --no-start) START=0 ;;
    --yes) ASSUME_YES=1 ;;
    -h | --help)
      sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "未知参数：$arg" >&2
      exit 2
      ;;
  esac
done

if [[ ! -f "$DB" ]]; then
  echo "找不到数据库：$DB" >&2
  echo "首次启动后才会创建，或 APP_DATA_DIR 指向了别的目录。" >&2
  exit 1
fi

if [[ "$ASSUME_YES" -eq 0 ]]; then
  echo "即将清空数据库：$DB"
  echo "将删除：项目/卷/章/角色/关系/道具/沙盒会话与事件/导入任务/LLM 用量/幂等记录"
  echo "将保留：app_config（LLM 配置）、prompt_templates"
  read -r -p "确认继续？[y/N] " reply
  case "$reply" in
    y | Y | yes | YES) ;;
    *)
      echo "已取消"
      exit 1
      ;;
  esac
fi

"$ROOT/scripts/serve.sh" stop >/dev/null 2>&1 || true

sqlite3 "$DB" <<'SQL'
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
DELETE FROM sandbox_events;
DELETE FROM sandbox_sessions;
DELETE FROM character_relations;
DELETE FROM items;
DELETE FROM chapters;
DELETE FROM volumes;
DELETE FROM characters;
DELETE FROM novels;
DELETE FROM novel_imports;
DELETE FROM search_docs;
DELETE FROM llm_usage;
DELETE FROM idempotency_records;
COMMIT;
PRAGMA wal_checkpoint(TRUNCATE);
SQL

echo "已清空业务数据：$DB"
sqlite3 "$DB" "PRAGMA foreign_keys = ON; VACUUM;"

if [[ "$START" -eq 1 ]]; then
  "$ROOT/scripts/serve.sh" start
fi
