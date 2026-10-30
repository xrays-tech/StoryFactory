# P6 — SSE 与完整会话 API

> 依赖：P5 | 门禁：@oracle
> 目标：SSE 线协议可靠（补发/心跳/Lagged 自愈/终结）、会话查询与 usage、waiting_input 完整闭环、abort 即时。

## 1. SSE（§6.4/§8.4/§9.6）
- `GET /api/v1/sandbox-sessions/{sid}/events`：
  - `after_seq` 参数与 `Last-Event-ID` 取大；
  - **先 subscribe broadcast → 再 DB 补发 `seq > n` → 按 seq 去重 → live**（D-20，避免订阅空窗丢事件）；
  - 心跳 15s `: ping`；客户端断开检测；
  - `Lagged`：以 `last_sent_seq` 重新查库补发；
  - 终结：补发后发 `sandbox.done` 并结束；
  - 响应头 no-cache / X-Accel-Buffering: no；**不经过压缩层**（D-20）。
- 0.0.0.0 模式：SSE 接受 `?token=`（D-26），`Referrer-Policy: no-referrer`。
- 未知事件类型前端兜底（P8）。

## 2. 会话 API
- `GET /api/v1/novels/{id}/sandbox-sessions?status=&page=`（A4）。
- `GET /api/v1/sandbox-sessions/{sid}` 快照（含 usage 聚合）。
- `POST /api/v1/sandbox-sessions/{sid}/control`（P5 已实现，P6 接入完整错误/响应）。
- `POST /api/v1/sandbox-sessions/{sid}/input`（D-14）。
- `POST /api/v1/chapters/{cid}/sandbox-sessions`、`POST /api/v1/novels/{id}/sandbox-sessions` 创建 + 幂等（D-07/D-21）。
- `finalize` 事件与 outline_ready。

## 3. 幂等中间件（D-21）
- 强制端点缺失 key → 422；可选端点自动生成并回传。
- 同键同体**纯回放**存储响应 + `Idempotency-Replayed: true`（不查活会话、不 UPDATE）；同键异体 409。
- 并发同键：主键冲突 → 回查回放。
- TTL 24h；清理 job。
- 写入与业务同事务（handler 内完成）。

## 4. 测试
- after_seq 重放、Last-Event-ID、心跳、Lagged 自愈（人为慢消费者）。
- 终结后连接关闭 + sandbox.done。
- abort 在 LLM 慢响应中（mock 延迟）：< 500ms 取消、无半步事件。
- waiting_input：input → running → 下一步 directive；300s 超时（用可配置短超时测试）→ finalizing。
- 幂等并发 2×POST 同 session。
- 会话列表筛选/分页。

## 5. 验收
```bash
cargo test --test sse
cargo test --test session_api
```
- curl 手工：`curl -N .../events?after_seq=0` 观察补发与 ping。

## 6. 决策引用
D-07、D-14、D-15、D-20、D-21。
