# StoryArchitect AI

本地优先、单文件分发的小说大纲创作工具。Rust/Axum 后端 + React 前端 + bundled SQLite（WAL/FTS5），数据不出本机，网络仅用于 LLM API。

## 构建与运行

```bash
# 后端（开发，前台运行，Ctrl-C 退出）
APP_DATA_DIR=./dev-data APP_HTTP__DEV_CORS=true cargo run

# 后端（脱离终端常驻，脚本管理：start|stop|restart|status|log）
scripts/serve.sh start          # 日志 /tmp/story-architect.log
scripts/serve.sh status
scripts/serve.sh stop

# 前端（开发，:5173 代理 /api → :8080）
npm --prefix frontend install
npm --prefix frontend run dev

# 生产构建
npm --prefix frontend run build
cargo build --release
```

- 默认监听 `127.0.0.1:8080`，数据目录遵循平台约定（macOS `~/Library/Application Support/StoryArchitect`、Linux `~/.local/share/story-architect`、Windows `%APPDATA%/StoryArchitect`），可用 `APP_DATA_DIR` 覆盖。
- 同一数据目录只允许一个实例（`story-architect.lock`）；重复启动会直接报错退出，避免多个进程对同一 WAL 各自持有视图。
- 用 `scripts/serve.sh` 启动时进程已脱离终端：命令立即返回，不会在任务面板里显示成「一直运行中」。
- 清空全部业务数据（项目/卷章/角色/道具/沙盒/导入任务/用量），保留 LLM 配置与提示词模板：
  ```bash
  scripts/reset-data.sh            # 停服 → 清空 → 重启
  scripts/reset-data.sh --no-start # 只清空
  scripts/reset-data.sh --yes      # 免确认
  ```
  必须停服后执行：服务运行期间用其它进程写同一个库会让新旧连接指向不同的 WAL inode。
- `APP_BIND=0.0.0.0` 时强制启动令牌（`app.token`，0600），首次访问用 `?token=`，前端存储后以 `X-App-Token` 发送。
- LLM 密钥只从环境变量读取：`APP_LLM_API_KEY`、`APP_LLM_FALLBACK_API_KEY_<N>`；不写入数据库/日志。

## 配置

优先级：`app_config(runtime_config)` > `APP_*` 环境变量 > 数据目录 `.env` > `config.toml` > 内置默认。示例见 `.env.example`、`config.example.toml`。运行期 `PATCH /api/v1/config` 白名单键持久化到 SQLite。

## 目录

```
src/            Rust 后端（config/db/error/models/services/llm/handlers/middleware/jobs）
migrations/     SQLite 迁移（sqlx::migrate! 编译期嵌入）
frontend/       Vite + React + TS + Tailwind
tests/          集成/契约测试（mock LLM，无网络）
dev-docs/       设计、审核裁决、分阶段计划
assets/prompts/ Prompt 模板（seed 到 DB）
```

## 测试

```bash
scripts/check.sh                      # 快速门：fmt + clippy + 定向集成测试
scripts/check.sh --full               # 提交前：再加全量 cargo test
scripts/check.sh --test novel_import  # 只跑某个集成测试
npm --prefix frontend run typecheck
npm --prefix frontend run build
```

`scripts/check.sh` 会实时打印每个阶段的输出与耗时，不重定向、不经过任何 shell 包装器。

注意：`clippy --all-targets` 与 `cargo test` 使用不同的编译产物指纹。首次在两者之间切换会整体重编（约 5–10 分钟，日志看似"卡住"），之后两边缓存同时命中，完整质量门约 15 秒。改动越小、越应该用 `--test <name>` 只跑相关套件。

## 安全

- 回环绑定：Host 白名单 + 非 GET/Origin 校验（防 DNS rebinding/CSRF）。
- 非回环绑定：32 位启动令牌；SSE 支持 `?token=`。
- XSS：Markdown 渲染 DOMPurify；SQL 全参数化。
- 导入/导出：zip + manifest + sha256 校验；导入单事务回滚。
- 日志：JSON 按日滚动，query token 脱敏；默认不记录 Prompt 全文。

## 已知限制

- 导入导出当前在内存中完成（本地量级可接受）。
- macOS 公证/Windows 签名需 CI 密钥，本地不执行。
- 前端 E2E 需浏览器环境；CI 矩阵见 `.github/workflows/`。

## 文档

- 设计基线：`dev-docs/00-design-v4.0.md`
- 审核与裁决（D-01..D-31）：`dev-docs/01-review-and-decisions.md`
- 分阶段计划：`dev-docs/02-development-plan.md`
