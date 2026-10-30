# P5 — 沙盒引擎核心

> 依赖：P2 / P4 | 门禁：@oracle（重点：状态机与并发）
> 目标：会话创建、状态机、三模式导演、Actor 执行、上下文组装/压缩/重建、事件落盘与 hub、崩溃恢复、control/input 端点。

## 1. 事件与模型
- `services/sandbox/events.rs`：`SandboxEventType` 全枚举（§8.7，D-06）；payload 结构体；`sandbox.done` 仅线上。
- `models/sandbox.rs`：`SandboxSessionRow`、`SessionSnapshot`、`UsageSummary`、`SessionError`。
- `models/enums.rs`：`SessionStatus` 9 态 + 迁移校验函数。

## 2. 状态机（§8.3）
- 集中 `transitions` 模块：`can_transition(from, action) -> Result<To, AppError>`；control handler 与 engine 共用。
- 全部迁移表单测（合法/非法组合）。
- `created→running`（autostart）、`paused→paused`（step 单步）实现（D-15）。

## 3. SessionManager / hub（§8.4）
- `DashMap<String, SessionHandle>`；`SessionHandle { cancel, task, hub, snapshot(watch), input_tx, permit }`。
- 事件双写：事务写 events + update session → `hub.send` → 检查令牌。
- `broadcast` 容量 256；`Lagged` 由 SSE 处理（P6）。
- 注册表仅活跃；终结移除并 drop hub。

## 4. 引擎（§8.1/8.2）
- 主循环：选角 → actor → 情感 → 事务落盘 → 广播 → 终止/暂停/取消检查 → 压缩检查 → 下一步。
- `director.rs`：roundrobin / director（LLM + Schema + 修复 + 非法降级轮转）/ scripted；防重复软提示 + 硬上限 `director_repeat_limit`；`terminate`/`request_user_input` 分支。
- `actor.rs`：单角色 Prompt 组装、LLM 调用、结构化校验、emotion 限幅、写 relations（归一化 a<b，自指跳过）。
- `context.rs`：token 布局（§7.5）、`est_tokens`、滚动摘要触发与落盘（`summary_until_seq`）、硬顶失败、恢复重建。
- `finalize`：outline_finalize LLM（失败则模板降级带 ⚠️）→ 写 `chapter.final_outline` + `summary_text` + 事件 + completed（chapter 缺失则仅 summary_text）。
- `single step`：RunMode::SingleStep。

## 5. 并发与恢复（§5.6/§8.5）
- `Semaphore` permits = max_concurrent；许可生命周期按 D-13；获取失败 429 + Retry-After（预估）。
- `CancellationToken` + `select!` 即时取消；半步不落盘。
- 启动恢复例程：活跃态 → interrupted + 事件（seq 续接）；`recover` 动作：校验、许可、重建、新 deadline、resumed 事件。
- `control`（start/pause/resume/step/abort/recover）与 `input` 端点（D-14）。

## 6. 后台任务
- 僵尸扫描 60s（running/finalizing 10min 无事件、paused/waiting 超 deadline → interrupted）。
- waiting_input 超时 300s → finalizing（引擎内 timer）。
- 幂等清理、回收站过期（P2 骨架接入）。

## 7. 测试（重点）
- 状态机全迁移矩阵。
- mock provider 引擎：roundrobin 跑满 step_limit；director 正常/非法降级/terminate/request_input；scripted 顺序；actor_error 熔断。
- 情感写入与夹紧；滚动摘要触发、`summary_until_seq` 推进、重建等价性。
- §8.8 场景 1（双订阅）、2（同键并发）、5（pause 竞争）、6（abort 即时）、7（并发 control）。
- 崩溃恢复：kill 模拟（直接调用恢复例程）+ recover 续跑。
- 单步：paused→执行一步→paused。

## 8. 验收
```bash
cargo test sandbox::
cargo test --test engine_scenarios
```
- 全部引擎测试用 mock，无网络；事件 seq 连续；无半步脏数据断言。

## 9. 决策引用
D-06、D-13、D-14、D-15、D-16。
