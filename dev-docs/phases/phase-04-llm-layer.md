# P4 — LLM 服务层

> 依赖：P1 | 门禁：@oracle
> 目标：可插拔 Provider、能力探测、故障转移、结构化输出修复回路、三层预算、模板渲染、成本落库。全部可用 mock 测试，无需真实网络。

## 1. 组件
- `llm/client.rs`：全局 `reqwest::Client`（rustls、gzip、http2、连接池）；`reqwest-middleware` + 自定义重试（仅网络/408/429/5xx）；Auth 注入与日志脱敏；`X-Request-Id`。
- `llm/providers/{deepseek,openai_compat}.rs`：`LlmProvider` trait 实现；`structured_mode()` 按 D-17；`complete()` 支持 `deadline`。
- `llm/failover.rs`：链路执行、429 Retry-After、每跳落 `llm_usage`（失败也记 error_kind）、全链失败返回 `LlmUnavailable`。
- `llm/structured.rs`：JSON 提取（容忍 markdown fence）→ `jsonschema` 校验 → 修复回路（≤2 轮，purpose=repair）→ 返回类型化结果或 `LlmInvalidOutput`。
- `llm/budget.rs`：`Budget { session_deadline, step_deadline, attempt_timeout }`，`attempt_deadline()` 计算；`remaining_ok()`；重试前置检查（D-18）。
- `llm/prompts.rs`：从 `prompt_templates` 读 active；minijinja Sandboxed；渲染四模板；模板版本快照 JSON；seed（D-19）。
- `llm/usage.rs`：`record_usage(tx, ...)`；pricing 从 `app_config.llm_pricing` 解析；缺失单价记 0 + 标记。
- `llm/mock.rs`（`#[cfg(test)]` 或 feature）：脚本化响应序列 + 录制回放（key = sha256(模板名+版本+角色ID+步号)）。

## 2. Schema
- 内置 ActorStepOutput / DirectorDecision JSON Schema（§7.4），作为常量 `serde_json::Value`；`structured` 校验使用。
- Schema 摘要注入（JsonObject 模式）。

## 3. 错误分类
- `LlmErrorKind`: network/timeout/rate_limited/invalid_output/auth/bad_request/server；映射到 `error_kind` 列与 `AppError`。

## 4. 测试
- 结构化：合法 JSON、fence 包裹、Schema 违反 → 修复成功、修复 2 轮失败。
- 故障转移：主 429 → fallback 成功；全链失败；401 立即失败不切链。
- 预算：attempt/step 耗尽路径；重试退避受预算约束。
- 模板：渲染快照、变量转义、未知变量报错；模板版本快照。
- usage：成功/失败均落库；pricing 缺失 cost=0。

## 5. 验收
```bash
cargo test llm::
cargo clippy --all-targets -- -D warnings
```
- 无真实密钥也可全绿；提供 `APP_LLM_API_KEY` 时手动 smoke 可选。

## 6. 决策引用
D-06、D-08、D-17、D-18、D-19。
