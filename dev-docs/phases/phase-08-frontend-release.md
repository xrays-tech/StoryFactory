# P8 — 前端完整功能与发布

> 依赖：P3 / P6 / P7 | 门禁：@oracle（含 @designer 终审）
> 目标：完整可用 UI、E2E 全链路、性能达标、发布流水线与文档。

## 1. 功能页面
- Dashboard：项目卡片、搜索、新建、回收站入口。
- NovelDetail：概览、角色、羁绊（力导向图/表格双视图）、卷章大纲树、物品、沙盒入口。
- SandboxPage：会话列表 + 创建向导（角色多选、模式、scene_prompt、step_limit、directive、autostart）。
- SandboxConsole 三栏（§9.4）：
  - 左：状态徽章 9 态、步数/预算倒计时、control 按钮组按迁移表禁用、input 框、成本轮询 30s。
  - 中：虚拟滚动 500 环形缓冲、角色着色、step 分组、导演折叠、摘要锚点、自动滚动暂停。
  - 右：outline 预览（DOMPurify）、保存定稿、降级标记 + 重试。
- Settings：LLM 配置（密钥掩码）、成本报表、备份导入导出、回收站、诊断、关于（版本三元组）。
- SSE 封装 `sse.ts`（§9.6）：退避重连、Last-Event-ID、45s 假死重建、未知事件兜底、done 关闭。
- 跨 store 订阅（§9.2）：outline_ready / emotion_updated / 保存 toast。
- 离开活跃会话确认框。

## 2. 设计（@designer 主导）
- 全站视觉一致性；控制台信息层级；响应式；暗色模式；动效克制（≤200ms）。
- 无障碍：焦点环、aria、键盘操作、对比度。
- 文案由 orchestrator 复核（不改视觉结构）。

## 3. 测试
- Vitest + RTL：stores、sse 重连/假死、错误码映射、控制台 9 态按钮。
- Playwright：黄金路径（建项目→角色→羁绊→沙盒 mock SSE→定稿→导出→回收站恢复）；断线重连；双标签页同看。
- 可访问性：axe 或 Lighthouse snapshot 基本项。

## 4. 性能与发布
- 性能基准：启动 healthz < 300ms、单会话重放 < 100ms、报表 < 500ms（集成计时）；内存人工观测。
- CI：`.github/workflows/ci.yml`（fmt/clippy/test/typecheck/build/契约新鲜度）、`release.yml` 原生 runner 矩阵（linux x64/arm64、win x64、mac x64/arm64），产物 + SHA-256。
- `scripts/release.sh`：工作区干净 → bump → CHANGELOG → tag。
- 文档：README（构建/运行/配置/安全说明/已知限制）、`dev-docs` 索引。
- 签名/公证标记"需 CI 密钥，未本地执行"。

## 5. 验收
```bash
npm --prefix frontend run typecheck && npm --prefix frontend run build
npm --prefix frontend run test
npx playwright test
cargo test
cargo build --release
```
- E2E 全绿；release 二进制可启动并服务嵌入 UI；关键指标记录。

## 6. 决策引用
D-20、D-24，以及 P3 设计交付记录。
