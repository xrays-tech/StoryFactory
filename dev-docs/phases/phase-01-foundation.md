# P1 — 基础骨架与安全底座

> 依赖：无 | 预计：1 个工作单元 | 门禁：@oracle
> 目标：让 `cargo run` 启动一个可访问、可测试、可迁移的 Axum 服务，安全中间件就位。

## 1. 范围

### 1.1 工程骨架
- `Cargo.toml`：依赖按设计 §2.1（axum 0.7、tokio、sqlx 0.8 sqlite bundled、serde、tracing、config、dotenvy、thiserror、uuid、dashmap、secrecy、rust-embed、tower-http、validator、jsonschema、reqwest 等按阶段引入；P1 只引入所需）。
- `rust-toolchain.toml`（stable）。
- `build.rs`：按 D-02 生成嵌入代码；release 严格校验。
- `frontend/dist/` 占位与 `static/fallback/index.html`。
- `migrations/0001_init.sql`：按设计 §4.1 + D-04（`replay_hint`）+ D-11（`deleted_batch`）+ D-14（`pending_directive`）+ D-22（`tokenize='trigram'`）修订；**不含自建 `schema_migrations` 表**（D-25）。
- `.env.example`、`config.example.toml`。

### 1.2 基础设施
- `config.rs`：多层加载（D-08），启动校验（模型名、超时关系、并发≤16、retention≥1、bind 合法），`AppConfig` + `ArcSwap`（或 `arc-swap` crate）。
- `error.rs`：`ErrorCode`（§5.3 + 新增码）、`AppError`、`IntoResponse`、`request_id`。
- `db.rs`：`SqliteConnectOptions`（WAL、busy_timeout 5s、foreign_keys ON、synchronous NORMAL）、`max_connections=4`、`sqlx::migrate!("./migrations")`（D-25，`_sqlx_migrations` 单事务 + checksum）、`begin_immediate()` helper、`PRAGMA user_version` 写入。
- `state.rs`：`AppState { config, db, sessions(占位), llm(占位) }`。
- `main.rs`：数据目录解析（平台默认）、日志初始化（JSON + 文件滚动）、启动恢复例程占位、`webbrowser` 打开、优雅关闭（Ctrl+C/SIGTERM，10s 宽限）。
- `router.rs` + 中间件链顺序（§5.4；SSE 路由留待 P6，压缩排除逻辑预留）。

### 1.3 安全底座
- `middleware/host_guard.rs`：Host 白名单（按 bind 端口动态生成）+ OriginGuard（非 GET/HEAD 的 Origin 校验）+ 0.0.0.0 令牌（`X-App-Token`，令牌文件 0600）+ 403/401。
- `middleware/request_log.rs`：request_id 注入、X-Request-Id 响应头、耗时日志。
- `middleware/body_limit.rs`：默认 2MB；为 P7 import 预留 64MB 路由级配置能力。

### 1.4 健康与测试
- `GET /api/healthz`、`GET /api/health`（DB 连通、`PRAGMA foreign_keys`、schema 版本、LLM 配置存在性——不发请求）。
- 测试：配置优先级、迁移幂等、HostGuard/OriginGuard 矩阵、health。
- `scripts/dev.sh`、`scripts/gen-types.sh` 占位。

## 2. 明确不做
- 业务 CRUD（P2）、LLM（P4）、沙盒（P5）、SSE（P6）、前端组件（P3）。

## 3. 验收（DoD）
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
# 启动后：
curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/api/healthz   # 200
curl -s http://127.0.0.1:8080/api/health                                  # {"status":"ok",...}
# Host 伪造
curl -s -H 'Host: evil.com' -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/api/healthz # 403
```
- 迁移在全新目录启动一次成功；重启不重复应用。
- `APP_BIND=0.0.0.0` 时无 token 请求 401，带正确 token 通过；UI 警告条留待 P3。

## 4. 关键决策引用
D-01（运行时查询）、D-02（build.rs）、D-03（版本）、D-06（枚举）、D-08（配置）、D-20（压缩排除预留）。

## 5. 交付文件（预估）
`Cargo.toml`, `build.rs`, `rust-toolchain.toml`, `migrations/0001_init.sql`, `src/{main,config,error,db,router,state}.rs`, `src/middleware/{host_guard,request_log,body_limit}.rs`, `src/handlers/health.rs`, `tests/{config,db,middleware}.rs`, `.env.example`, `config.example.toml`.
