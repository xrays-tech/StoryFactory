#!/usr/bin/env bash
# P1 临时类型占位；P3 起由 ts-rs 测试导出替换（ADR-07）
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p frontend/src/types/generated
cat > frontend/src/types/generated/bootstrap.ts <<'TS'
// 临时占位：P3 生成真实类型后删除
export interface HealthResponse {
  status: string;
  app_version: string;
  schema_version: number;
  format_version: number;
  db: string;
  llm_configured: boolean;
}
export interface ErrorEnvelope {
  error: { code: string; message: string; details?: unknown; request_id: string };
}
TS
echo "frontend/src/types/generated/bootstrap.ts generated (placeholder)"
