# P7 — 导入导出 / 搜索 / 设置 / 成本

> 依赖：P2 / P4 / P6 | 门禁：@oracle
> 目标：项目可移植（附录 C）、全文检索可用、运行期配置热更新、成本报表与诊断、整库备份。

## 1. 导出（附录 C）
- `GET /api/v1/novels/{id}/export?include_sessions=&include_deleted=`。
- zip：`manifest.json` + `data.json`；`data.json` 流式写临时文件；checksum SHA-256；counts。
- `format_version=2`、`schema_version` 动态读取 `_sqlx_migrations`（D-03/D-25）、`app_version`。
- 文件名白名单 + `Content-Disposition: attachment`。
- 导出含明文设定、不含密钥（提示）。

## 2. 导入
- `POST /api/v1/import`，路由级 64MB（D-04/§5.4）。
- 校验 checksum → format_version（>2 → `UnsupportedImportVersion`）→ ID 重映射 → 唯一性预处理（`（导入）`/序号平移）→ 单事务写 → counts 对账。
- 重写引用：chapters.volume_id、relations、items.owner_id、sessions.chapter_id/actor_ids、events.session_id；**并重写事件 payload 内 actor_id/next_actor_id/pair/target_actor_id 与 director_config[].actor_id（D-27）**。
- `llm_usage` 不导入。
- 失败全回滚；返回导入报告。
- 幂等可选（D-21）。

## 3. 搜索（D-22）
- `GET /api/v1/search?q=&type=&novel_id=&page=`；q≥2；`"{q}*"`；`snippet/highlight`；bm25 排序。
- 校验 FTS 同步（P2 已实现，此处暴露端点 + 测试）。

## 4. 设置与成本
- `GET/PATCH /api/v1/config`（白名单 D-08；密钥掩码）。
- `GET /api/v1/usage?month=YYYY-MM`：按日聚合 + purpose 分布 + 单价缺失标注。
- `POST /api/v1/client-errors`（32KB、日志、204）。
- 整库备份：启动时 `wal_checkpoint(TRUNCATE)` + 复制，保留 3 份；恢复流程（先备份当前库）。
- 诊断面板数据（日志路径、磁盘占用、LLM 成功率/延迟分位）——后端聚合端点或读取既有 usage。
- usage 聚合 job（5min）。

## 5. 测试
- 导出→导入往返：实体计数一致、ID 全换新、引用正确、FTS 可搜。
- checksum 损坏 → 422；format_version=99 → 422；旧版本映射（保留桩）。
- 导入事务失败回滚（构造冲突）。
- 搜索中文/英文/类型过滤/软删排除。
- config PATCH 白名单 + 热生效（并发读）。
- usage 聚合口径；client-errors 32KB 截断。

## 6. 验收
```bash
cargo test --test export_import
cargo test --test search
cargo test --test config_usage
```
- 手工：导出 zip → 新库导入 → 大纲/角色/会话齐全。

## 7. 决策引用
D-03、D-04、D-08、D-12、D-21、D-22、D-23。
