# P3 — 契约生成与前端骨架

> 依赖：P2 | 门禁：@oracle（含 @designer 设计意图评审）
> 目标：Rust↔TS 类型链路打通；前端可运行、可导航、可完成"建项目→建角色→建卷章→删除→恢复"骨架流程。

## 1. 契约链路（§6.1 / ADR-07）
- `tests/export_bindings.rs`：调用 `ts_rs::export_all_to("../frontend/src/types/generated")`（或逐类型 `export_to`）；`TS_RS_EXPORT_DIR` 锁定。
- `frontend/src/types/generated/` 产出提交入 git（`.gitignore` 当前忽略，需在 P3 移除此条并提交产物——**注意：本仓库计划要求产物入 git；实现时从 .gitignore 移除该行**）。
- `package.json` 脚本：`gen:types = cd .. && cargo test --test export_bindings`、`typecheck = tsc --noEmit`。
- `frontend/src/api/endpoints.ts`：路径常量与请求函数（与 §6.2 一一对应）。
- `client.ts`：fetch 封装（超时、错误规范化、Idempotency-Key 生成、X-App-Token、X-Request-Id 透传）。
- `tests/contract.rs`：路由表断言（可用静态列表对比）。

## 2. 前端工程
- Vite 5 + React 18 + TS + Tailwind 3.4 + React Router 6 数据路由。
- `src/main.tsx`、`router.tsx`、`AppLayout`（侧边栏/顶栏/内容区）。
- Tailwind 主题 token：颜色/间距/字体/圆角；暗色模式（`uiStore.theme`）。
- 基础组件（designer 主导）：Button、Input、Textarea、Select、Dialog、ConfirmDialog、Toast、Tabs、Table、EmptyState、Badge、Skeleton、ErrorBoundary。
- 页面骨架：Dashboard（项目卡片/搜索/新建）、NovelDetail 容器 + tabs（概览/角色/羁绊/卷章/物品/沙盒占位）、Settings 占位。
- stores：`novelStore`、`characterStore`、`outlineStore`、`uiStore`（Zustand + immer + subscribeWithSelector）。
- 表单：前端校验即时反馈；服务端 422 details inline。
- 文案中文，集中 `src/copy/zh.ts`。

## 3. 设计交付要求（@designer）
- 视觉方向：文学创作工具的沉静感，避免"后台管理模板"观感；中性暖色底 + 单一强调色；信息密度适中。
- 三档响应式：≥1280 三栏/双栏、768–1279 折叠侧栏、<768 单列。
- 交互：键盘可达、焦点可见、loading/empty/error 三态齐全。
- 交付后 orchestrator 只修文案，不改视觉结构。

## 4. 测试
- `npm run typecheck`、`npm run build`。
- Vitest：stores、client 错误映射、sse 封装（P6 接入）。
- Playwright 骨架：建项目 → 建角色 → 建卷章 → 删除章节 → 回收站恢复。
- 双端启动脚本 `scripts/dev.sh`（前端 5173 代理 /api → 8080）。

## 5. 验收
```bash
npm run gen:types && git diff --exit-code -- frontend/src/types/generated
npm --prefix frontend run typecheck
npm --prefix frontend run build
cargo test --test export_bindings
```
- 前端可完成骨架黄金路径；错误 Toast 可展示 `error.code`。

## 6. 决策引用
D-06、D-09、D-16（快照类型预留）、D-24。
