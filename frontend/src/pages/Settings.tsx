import { useEffect, useRef, useState } from "react";
import type { LoaderFunctionArgs } from "react-router-dom";
import { useLoaderData, useNavigate, useRevalidator, useSearchParams } from "react-router-dom";
import {
  BarChart3,
  BookOpenText,
  CheckCircle2,
  Database,
  Download,
  FileArchive,
  HardDrive,
  Info,
  KeyRound,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Upload,
  XCircle,
} from "lucide-react";
import type { AppConfig, HealthReport, ImportReport, UsageReport } from "../types/api";
import type { NovelDto } from "../types/domain";
import type { Wire } from "../types/api";
import * as api from "../api/endpoints";
import { getAppToken } from "../api/client";
import { useUiStore } from "../stores/uiStore";
import { zh } from "../copy/zh";
import { userMessage } from "../lib/errors";
import { bytesToSize, downloadBlob, formatCost, formatNumber } from "../lib/format";
import { PageHeader } from "../components/common/PageHeader";
import { Tabs, type TabItem } from "../components/common/Tabs";
import { Button } from "../components/common/Button";
import { Field } from "../components/common/Field";
import { Input, Select } from "../components/common/Input";
import { Badge } from "../components/common/Badge";
import { EmptyState, ErrorState } from "../components/common/EmptyState";
import { LoadingBlock } from "../components/common/Skeleton";
import { ProgressBar } from "../components/common/Progress";
import { RecycleBinPanel } from "../components/settings/RecycleBinPanel";
import { cn } from "../lib/cn";

type SettingsTab = "llm" | "usage" | "data" | "recycle" | "about";

export type SettingsLoaderData = {
  config: AppConfig | null;
  configError: string | null;
  usage: UsageReport | null;
  usageError: string | null;
  health: HealthReport | null;
  healthError: string | null;
  novels: Array<Wire<NovelDto>>;
  novelsError: string | null;
};

export async function settingsLoader(_args: LoaderFunctionArgs): Promise<SettingsLoaderData> {
  const [config, usage, health, novels] = await Promise.allSettled([
    api.getConfig(),
    api.getUsage(),
    api.getHealth(),
    api.listNovels({ page: 1, pageSize: 100 }),
  ]);
  return {
    config: config.status === "fulfilled" ? config.value : null,
    configError: config.status === "rejected" ? userMessage(config.reason) : null,
    usage: usage.status === "fulfilled" ? usage.value : null,
    usageError: usage.status === "rejected" ? userMessage(usage.reason) : null,
    health: health.status === "fulfilled" ? health.value : null,
    healthError: health.status === "rejected" ? userMessage(health.reason) : null,
    novels: novels.status === "fulfilled" ? novels.value.items : [],
    novelsError: novels.status === "rejected" ? userMessage(novels.reason) : null,
  };
}

function currentMonth(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
}

const tabs: Array<TabItem<SettingsTab>> = [
  { value: "llm", label: <><Settings2 className="h-3.5 w-3.5" /> {zh.settings.llm}</> },
  { value: "usage", label: <><BarChart3 className="h-3.5 w-3.5" /> {zh.settings.usage}</> },
  { value: "data", label: <><Database className="h-3.5 w-3.5" /> {zh.settings.data}</> },
  { value: "recycle", label: zh.settings.recycle },
  { value: "about", label: <><Info className="h-3.5 w-3.5" /> {zh.settings.about}</> },
];

export function SettingsPage() {
  const data = useLoaderData() as SettingsLoaderData;
  const [searchParams, setSearchParams] = useSearchParams();
  const tabParam = searchParams.get("tab");
  const tab: SettingsTab = (["llm", "usage", "data", "recycle", "about"] as const).includes(
    tabParam as SettingsTab,
  )
    ? (tabParam as SettingsTab)
    : "llm";

  const onChange = (next: SettingsTab) => {
    setSearchParams(next === "llm" ? {} : { tab: next });
  };

  return (
    <div className="space-y-5">
      <PageHeader
        eyebrow="本地工作台"
        title={zh.settings.title}
        description={zh.settings.subtitle}
        actions={<Badge tone="success" dot>本地运行</Badge>}
      >
        <Tabs value={tab} onChange={onChange} items={tabs} className="mt-1" />
      </PageHeader>

      {data.configError && tab === "llm" ? (
        <ConfigError message={data.configError} />
      ) : tab === "llm" ? (
        <LlmSettings initial={data.config} health={data.health} healthError={data.healthError} />
      ) : null}

      {tab === "usage" ? (
        <UsagePanel initial={data.usage} initialError={data.usageError} />
      ) : null}

      {tab === "data" ? (
        <DataPanel novels={data.novels} novelsError={data.novelsError} />
      ) : null}

      {tab === "recycle" ? <RecycleBinPanel /> : null}

      {tab === "about" ? (
        <AboutPanel health={data.health} healthError={data.healthError} config={data.config} />
      ) : null}
    </div>
  );
}

function ConfigError({ message }: { message: string }) {
  const revalidator = useRevalidator();
  return (
    <ErrorState
      title="配置读取失败"
      message={message}
      onRetry={() => revalidator.revalidate()}
    />
  );
}

type LlmForm = {
  provider: string;
  baseUrl: string;
  model: string;
  fallbacks: string;
  maxConcurrent: number;
  budget: number;
  heartbeat: number;
  logLevel: string;
  attemptTimeout: number;
  stepTimeout: number;
  sessionTimeout: number;
  retention: number;
};

function formFromConfig(config: AppConfig): LlmForm {
  return {
    provider: config.llm.provider,
    baseUrl: config.llm.base_url,
    model: config.llm.model,
    fallbacks: config.llm.fallbacks.join(", "),
    maxConcurrent: config.sandbox.max_concurrent,
    budget: config.cost.monthly_budget_usd,
    heartbeat: config.http.sse_heartbeat_seconds,
    logLevel: config.log.level,
    attemptTimeout: config.llm.attempt_timeout_seconds,
    stepTimeout: config.sandbox.step_timeout_seconds,
    sessionTimeout: config.sandbox.session_timeout_seconds,
    retention: config.recycle.retention_days,
  };
}

function LlmSettings({
  initial,
  health,
  healthError,
}: {
  initial: AppConfig | null;
  health: HealthReport | null;
  healthError: string | null;
}) {
  const toast = useUiStore((s) => s.toast);
  const [form, setForm] = useState<LlmForm | null>(initial ? formFromConfig(initial) : null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (initial) setForm(formFromConfig(initial));
  }, [initial]);

  if (!initial || !form) {
    return <LoadingBlock label="正在读取配置…" />;
  }
  const config = initial;

  const save = async () => {
    const errors: Record<string, string> = {};
    if (!form.model.trim()) errors.model = "模型不能为空";
    if (form.maxConcurrent < 1 || form.maxConcurrent > 16) errors.max_concurrent = "并发必须在 1–16 之间";
    if (form.heartbeat < 1) errors.sse_heartbeat_seconds = "心跳至少 1 秒";
    if (form.attemptTimeout < 1) errors.attempt_timeout_seconds = "超时必须大于 0";
    if (form.stepTimeout < 1) errors.step_timeout_seconds = "超时必须大于 0";
    if (form.sessionTimeout < 1) errors.session_timeout_seconds = "超时必须大于 0";
    if (form.budget < 0) errors.monthly_budget_usd = "预算不能为负";
    setFieldErrors(errors);
    if (Object.keys(errors).length > 0) return;

    setSaving(true);
    setError(null);
    try {
      const updated = await api.patchConfig({
        provider: form.provider.trim(),
        base_url: form.baseUrl.trim(),
        model: form.model.trim(),
        fallbacks: form.fallbacks
          .split(",")
          .map((item) => item.trim())
          .filter(Boolean),
        max_concurrent: form.maxConcurrent,
        monthly_budget_usd: form.budget,
        sse_heartbeat_seconds: form.heartbeat,
        log_level: form.logLevel,
        attempt_timeout_seconds: form.attemptTimeout,
        step_timeout_seconds: form.stepTimeout,
        session_timeout_seconds: form.sessionTimeout,
        retention_days: form.retention,
      });
      setForm(formFromConfig(updated));
      toast({ tone: "success", message: zh.settings.savedLlm });
    } catch (err) {
      setError(userMessage(err));
      setFieldErrors((err as { fieldErrors?: Record<string, string> }).fieldErrors ?? {});
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
      <section className="surface-card p-5">
        <div className="mb-4 flex items-center justify-between gap-2">
          <div>
            <h2 className="font-display text-base font-semibold text-ink">{zh.settings.llm}</h2>
            <p className="mt-1 text-xs text-muted">热更新会写入本地数据库，并立即影响新的 LLM 调用。</p>
          </div>
          <Button variant="primary" leftIcon={<CheckCircle2 className="h-4 w-4" />} loading={saving} onClick={() => void save()}>
            {zh.settings.saveLlm}
          </Button>
        </div>

        {config.warnings && config.warnings.length > 0 ? (
          <div className="mb-4 rounded-lg border border-warning/30 bg-warning-soft/50 px-3 py-2 text-[11px] leading-5 text-warning">
            {config.warnings.map((warning) => (
              <p key={warning}>· {warning}</p>
            ))}
          </div>
        ) : null}

        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={zh.settings.provider} htmlFor="cfg-provider">
            <Input
              id="cfg-provider"
              value={form.provider}
              onChange={(e) => setForm({ ...form, provider: e.target.value })}
              placeholder="deepseek"
            />
          </Field>
          <Field label={zh.settings.model} htmlFor="cfg-model" error={fieldErrors.model}>
            <Input
              id="cfg-model"
              value={form.model}
              onChange={(e) => setForm({ ...form, model: e.target.value })}
              invalid={!!fieldErrors.model}
              placeholder="deepseek-chat"
            />
          </Field>
          <Field label={zh.settings.baseUrl} htmlFor="cfg-base" className="sm:col-span-2">
            <Input
              id="cfg-base"
              value={form.baseUrl}
              onChange={(e) => setForm({ ...form, baseUrl: e.target.value })}
              placeholder="https://api.deepseek.com/v1"
            />
          </Field>
          <Field label={zh.settings.fallbacks} htmlFor="cfg-fallbacks" hint={zh.settings.fallbackHint} className="sm:col-span-2">
            <Input
              id="cfg-fallbacks"
              value={form.fallbacks}
              onChange={(e) => setForm({ ...form, fallbacks: e.target.value })}
              placeholder="openai:gpt-4o-mini, ollama:qwen2.5"
            />
          </Field>
        </div>

        <div className="mt-5 border-t border-line/60 pt-4">
          <h3 className="text-xs font-semibold text-ink">运行参数</h3>
          <div className="mt-3 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
            <Field label={zh.settings.concurrency} htmlFor="cfg-concurrency" error={fieldErrors.max_concurrent}>
              <Input
                id="cfg-concurrency"
                type="number"
                min={1}
                max={16}
                value={form.maxConcurrent}
                onChange={(e) => setForm({ ...form, maxConcurrent: Number(e.target.value) })}
                invalid={!!fieldErrors.max_concurrent}
              />
            </Field>
            <Field label={zh.settings.budget} htmlFor="cfg-budget" error={fieldErrors.monthly_budget_usd}>
              <Input
                id="cfg-budget"
                type="number"
                min={0}
                step="0.01"
                value={form.budget}
                onChange={(e) => setForm({ ...form, budget: Number(e.target.value) })}
                invalid={!!fieldErrors.monthly_budget_usd}
              />
            </Field>
            <Field label={zh.settings.heartbeat} htmlFor="cfg-heartbeat" error={fieldErrors.sse_heartbeat_seconds}>
              <Input
                id="cfg-heartbeat"
                type="number"
                min={1}
                value={form.heartbeat}
                onChange={(e) => setForm({ ...form, heartbeat: Number(e.target.value) })}
                invalid={!!fieldErrors.sse_heartbeat_seconds}
              />
            </Field>
            <Field label={zh.settings.attemptTimeout} htmlFor="cfg-attempt" error={fieldErrors.attempt_timeout_seconds}>
              <Input
                id="cfg-attempt"
                type="number"
                min={1}
                value={form.attemptTimeout}
                onChange={(e) => setForm({ ...form, attemptTimeout: Number(e.target.value) })}
                invalid={!!fieldErrors.attempt_timeout_seconds}
              />
            </Field>
            <Field label={zh.settings.stepTimeout} htmlFor="cfg-step" error={fieldErrors.step_timeout_seconds}>
              <Input
                id="cfg-step"
                type="number"
                min={1}
                value={form.stepTimeout}
                onChange={(e) => setForm({ ...form, stepTimeout: Number(e.target.value) })}
                invalid={!!fieldErrors.step_timeout_seconds}
              />
            </Field>
            <Field label={zh.settings.sessionTimeout} htmlFor="cfg-session" error={fieldErrors.session_timeout_seconds}>
              <Input
                id="cfg-session"
                type="number"
                min={1}
                value={form.sessionTimeout}
                onChange={(e) => setForm({ ...form, sessionTimeout: Number(e.target.value) })}
                invalid={!!fieldErrors.session_timeout_seconds}
              />
            </Field>
            <Field label={zh.settings.logLevel} htmlFor="cfg-log">
              <Select
                id="cfg-log"
                value={form.logLevel}
                onChange={(e) => setForm({ ...form, logLevel: e.target.value })}
              >
                {["trace", "debug", "info", "warn", "error"].map((level) => (
                  <option key={level} value={level}>
                    {level}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label={zh.settings.retention} htmlFor="cfg-retention">
              <Input
                id="cfg-retention"
                type="number"
                min={0}
                value={form.retention}
                onChange={(e) => setForm({ ...form, retention: Number(e.target.value) })}
              />
            </Field>
          </div>
        </div>

        {error ? (
          <p className="mt-4 rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm text-danger" role="alert">
            {error}
          </p>
        ) : null}
      </section>

      <aside className="space-y-3">
        <section className="surface-card p-4">
          <h2 className="flex items-center gap-1.5 font-display text-sm font-semibold text-ink">
            <KeyRound className="h-3.5 w-3.5 text-accent" /> {zh.settings.apiKey}
          </h2>
          <p className="mt-2 font-mono text-sm tracking-widest text-muted">••••••••••••••••</p>
          <div className="mt-2 flex items-center gap-2">
            {health?.llm_configured ? (
              <Badge tone="success" dot>{zh.settings.apiKeyConfigured}</Badge>
            ) : (
              <Badge tone="warning" dot>{zh.settings.apiKeyMissing}</Badge>
            )}
            <span className="text-[10px] text-faint">{zh.settings.apiKeyMasked}</span>
          </div>
          {healthError ? <p className="mt-2 text-[10px] leading-5 text-danger">{healthError}</p> : null}
          <p className="mt-3 rounded-lg bg-paper/50 px-2.5 py-2 text-[10px] leading-5 text-faint">
            密钥只通过环境变量 APP_LLM_API_KEY 注入，接口返回不包含明文。若未配置，沙盒启动会返回 llm_config_missing。
          </p>
        </section>

        <section className="surface-card p-4">
          <h2 className="font-display text-sm font-semibold text-ink">当前绑定</h2>
          <dl className="mt-2 space-y-1.5 text-[11px]">
            <div className="flex justify-between gap-2">
              <dt className="text-faint">地址</dt>
              <dd className="text-right font-mono text-muted">{config.bind}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt className="text-faint">数据库</dt>
              <dd className="text-right text-muted">{health?.db ?? "—"}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt className="text-faint">应用版本</dt>
              <dd className="text-right font-mono text-muted">{health?.app_version ?? "4.0.0"}</dd>
            </div>
          </dl>
        </section>
      </aside>
    </div>
  );
}

function UsagePanel({
  initial,
  initialError,
}: {
  initial: UsageReport | null;
  initialError: string | null;
}) {
  const [month, setMonth] = useState(initial?.month ?? currentMonth());
  const [report, setReport] = useState<UsageReport | null>(initial);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(initialError);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await api.getUsage(month);
      setReport(data);
    } catch (err) {
      setError(userMessage(err));
    } finally {
      setLoading(false);
    }
  };

  const days = report?.by_day ?? [];
  const maxCost = Math.max(0.0001, ...days.map((day) => day.cost_usd));
  const totalCalls = report?.total_calls ?? 0;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <Field label={zh.settings.month} htmlFor="usage-month">
          <Input
            id="usage-month"
            type="month"
            value={month}
            onChange={(e) => setMonth(e.target.value)}
            className="w-44"
          />
        </Field>
        <Button variant="outline" leftIcon={<RefreshCw className="h-4 w-4" />} loading={loading} onClick={() => void load()}>
          查询
        </Button>
      </div>

      {error ? (
        <ErrorState title="报表读取失败" message={error} onRetry={() => void load()} compact />
      ) : loading && !report ? (
        <LoadingBlock label="正在汇总成本…" />
      ) : !report || report.total_calls === 0 ? (
        <EmptyState
          icon={<BarChart3 className="h-5 w-5" />}
          title={zh.settings.noUsage}
          description="发起沙盒会话或让 LLM 参与创作后，用量会按月汇总到这里。"
        />
      ) : (
        <>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
            <div className="surface-card p-4">
              <p className="text-[11px] text-faint">{zh.settings.totalCost}</p>
              <p className="mt-1 font-display text-2xl font-semibold text-ink">{formatCost(report.total_cost_usd)}</p>
            </div>
            <div className="surface-card p-4">
              <p className="text-[11px] text-faint">{zh.settings.totalCalls}</p>
              <p className="mt-1 font-display text-2xl font-semibold text-ink">{formatNumber(report.total_calls)}</p>
            </div>
            <div className="surface-card p-4">
              <p className="text-[11px] text-faint">活跃天数</p>
              <p className="mt-1 font-display text-2xl font-semibold text-ink">{report.by_day.length}</p>
            </div>
            <div className="surface-card p-4">
              <p className="text-[11px] text-faint">单次均价</p>
              <p className="mt-1 font-display text-2xl font-semibold text-ink">
                {formatCost(report.total_cost_usd / Math.max(1, report.total_calls))}
              </p>
            </div>
          </div>

          <section className="surface-card p-5">
            <h2 className="font-display text-base font-semibold text-ink">{zh.settings.byDay}</h2>
            <div className="mt-4 flex h-40 items-end gap-1.5">
              {days.map((day) => (
                <div key={day.date} className="group flex h-full min-w-0 flex-1 flex-col justify-end" title={`${day.date} · ${formatCost(day.cost_usd)} · ${day.calls} 次`}>
                  <div
                    className="w-full rounded-t bg-accent/70 transition-colors group-hover:bg-accent"
                    style={{ height: `${Math.max(3, (day.cost_usd / maxCost) * 100)}%` }}
                    aria-label={`${day.date} ${formatCost(day.cost_usd)}`}
                  />
                </div>
              ))}
            </div>
            <div className="mt-2 flex justify-between text-[10px] text-faint">
              <span>{days[0]?.date}</span>
              <span>{days[days.length - 1]?.date}</span>
            </div>
          </section>

          <section className="surface-card p-5">
            <h2 className="font-display text-base font-semibold text-ink">{zh.settings.byPurpose}</h2>
            <div className="mt-3 space-y-2.5">
              {report.by_purpose.map((purpose) => (
                <div key={purpose.purpose}>
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-muted">{purpose.purpose}</span>
                    <span className="font-mono text-[11px] text-faint">
                      {purpose.calls} · {Math.round((purpose.calls / Math.max(1, totalCalls)) * 100)}%
                    </span>
                  </div>
                  <ProgressBar value={purpose.calls} max={Math.max(1, totalCalls)} className="mt-1" tone="info" />
                </div>
              ))}
            </div>
          </section>
        </>
      )}
    </div>
  );
}

function DataPanel({
  novels,
  novelsError,
}: {
  novels: Array<Wire<NovelDto>>;
  novelsError: string | null;
}) {
  const toast = useUiStore((s) => s.toast);
  const navigate = useNavigate();
  const fileRef = useRef<HTMLInputElement>(null);
  const [novelId, setNovelId] = useState(novels[0]?.id ?? "");
  const [includeSessions, setIncludeSessions] = useState(true);
  const [includeDeleted, setIncludeDeleted] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [importName, setImportName] = useState<string | null>(null);

  const doExport = async () => {
    if (!novelId) return;
    setExporting(true);
    setError(null);
    try {
      const { blob, filename } = await api.exportNovel(novelId, { includeSessions, includeDeleted });
      const fallback = `${novels.find((novel) => novel.id === novelId)?.title ?? "novel"}.zip`;
      downloadBlob(blob, filename ?? fallback);
      toast({ tone: "success", message: "导出已开始下载" });
    } catch (err) {
      setError(userMessage(err));
      toast({ tone: "error", message: userMessage(err) });
    } finally {
      setExporting(false);
    }
  };

  const doImport = async (file: File | null) => {
    if (!file) return;
    setImporting(true);
    setError(null);
    setReport(null);
    setImportName(file.name);
    try {
      const result = await api.importPackage(file);
      setReport(result);
      toast({ tone: "success", message: zh.settings.importDone });
    } catch (err) {
      setError(userMessage(err));
      toast({ tone: "error", message: userMessage(err) });
    } finally {
      setImporting(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <section className="surface-card p-5">
        <div className="flex items-center gap-2">
          <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-accent-soft text-accent-strong dark:text-accent">
            <Download className="h-4 w-4" />
          </span>
          <div>
            <h2 className="font-display text-base font-semibold text-ink">{zh.settings.exportTitle}</h2>
            <p className="mt-0.5 text-[11px] text-muted">{zh.settings.exportHint}</p>
          </div>
        </div>

        {novelsError ? <p className="mt-3 text-xs text-danger">{novelsError}</p> : null}
        <div className="mt-4 space-y-3">
          <Field label="选择项目" htmlFor="export-novel">
            <Select id="export-novel" value={novelId} onChange={(e) => setNovelId(e.target.value)}>
              {novels.length === 0 ? <option value="">没有可导出的项目</option> : null}
              {novels.map((novel) => (
                <option key={novel.id} value={novel.id}>
                  {novel.title}
                </option>
              ))}
            </Select>
          </Field>
          <label className="flex items-center gap-2 text-sm text-muted">
            <input
              type="checkbox"
              checked={includeSessions}
              onChange={(e) => setIncludeSessions(e.target.checked)}
              className="h-4 w-4 rounded border-line text-accent focus:ring-accent"
            />
            {zh.settings.includeSessions}
          </label>
          <label className="flex items-center gap-2 text-sm text-muted">
            <input
              type="checkbox"
              checked={includeDeleted}
              onChange={(e) => setIncludeDeleted(e.target.checked)}
              className="h-4 w-4 rounded border-line text-accent focus:ring-accent"
            />
            {zh.settings.includeDeleted}
          </label>
          <Button
            variant="primary"
            leftIcon={<FileArchive className="h-4 w-4" />}
            loading={exporting}
            disabled={!novelId}
            onClick={() => void doExport()}
          >
            {exporting ? zh.settings.exporting : zh.settings.export}
          </Button>
        </div>
      </section>

      <section className="surface-card p-5">
        <div className="flex items-center gap-2">
          <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-info-soft text-info">
            <Upload className="h-4 w-4" />
          </span>
          <div>
            <h2 className="font-display text-base font-semibold text-ink">{zh.settings.importTitle}</h2>
            <p className="mt-0.5 text-[11px] text-muted">{zh.settings.importHint}</p>
          </div>
        </div>

        <input
          ref={fileRef}
          type="file"
          accept=".zip,application/zip"
          className="sr-only"
          onChange={(e) => void doImport(e.target.files?.[0] ?? null)}
        />
        <div
          className="mt-4 flex flex-col items-center justify-center rounded-xl border border-dashed border-line/90 bg-paper/40 px-4 py-8 text-center"
          onDragOver={(event) => event.preventDefault()}
          onDrop={(event) => {
            event.preventDefault();
            void doImport(event.dataTransfer.files?.[0] ?? null);
          }}
        >
          <Upload className="h-6 w-6 text-faint" />
          <p className="mt-2 text-sm text-muted">拖入 zip 包，或</p>
          <Button
            variant="outline"
            size="sm"
            className="mt-2"
            loading={importing}
            onClick={() => fileRef.current?.click()}
          >
            {importing ? zh.settings.importing : zh.settings.import}
          </Button>
          {importName ? <p className="mt-2 font-mono text-[10px] text-faint">{importName}</p> : null}
        </div>

        {report ? (
          <div className="mt-4 rounded-xl border border-success/25 bg-success-soft/40 p-3">
            <div className="flex items-center gap-1.5">
              <CheckCircle2 className="h-4 w-4 text-success" />
              <p className="text-sm font-medium text-success">{zh.settings.importReport}</p>
            </div>
            <ul className="mt-2 grid grid-cols-2 gap-1 text-[11px] text-muted">
              {Object.entries(report.imported).map(([key, value]) => (
                <li key={key} className="flex justify-between gap-2 rounded bg-raised px-2 py-1">
                  <span>{key}</span>
                  <span className="font-mono text-ink">{String(value)}</span>
                </li>
              ))}
            </ul>
            {report.renamed.length > 0 ? (
              <p className="mt-2 text-[10px] leading-5 text-warning">重命名：{report.renamed.join("、")}</p>
            ) : null}
            {report.skipped.length > 0 ? (
              <p className="mt-1 text-[10px] leading-5 text-faint">跳过：{report.skipped.join("、")}</p>
            ) : null}
            <div className="mt-2 flex flex-wrap gap-2">
              <Button size="sm" variant="outline" leftIcon={<BookOpenText className="h-3.5 w-3.5" />} onClick={() => navigate(`/novel/${report.novel_id}`)}>
                打开导入的项目
              </Button>
              <span className="pt-1.5 font-mono text-[10px] text-faint">#{report.novel_id.slice(0, 8)}</span>
            </div>
          </div>
        ) : null}

        {error ? (
          <p className="mt-3 rounded-lg border border-danger/25 bg-danger-soft/50 px-3 py-2 text-sm text-danger" role="alert">
            {error}
          </p>
        ) : null}
      </section>
    </div>
  );
}

function AboutPanel({
  health,
  healthError,
  config,
}: {
  health: HealthReport | null;
  healthError: string | null;
  config: AppConfig | null;
}) {
  const token = getAppToken();
  return (
    <div className="grid gap-4 lg:grid-cols-[1.3fr_1fr]">
      <section className="surface-card p-5">
        <div className="flex items-start gap-3">
          <span className="flex h-12 w-12 items-center justify-center rounded-2xl bg-accent text-white shadow-sm dark:text-[#201612]">
            <BookOpenText className="h-6 w-6" />
          </span>
          <div>
            <h2 className="font-display text-xl font-semibold text-ink">{zh.appName} AI</h2>
            <p className="mt-1 text-sm leading-6 text-muted">{zh.settings.aboutText}</p>
          </div>
        </div>

        <div className="mt-5 grid gap-2 sm:grid-cols-3">
          {[
            { label: zh.settings.version, value: health?.app_version ?? "—", icon: Info },
            { label: zh.settings.schema, value: health ? `v${health.schema_version}` : "—", icon: Database },
            { label: zh.settings.format, value: health ? `v${health.format_version}` : "—", icon: HardDrive },
          ].map((item) => {
            const Icon = item.icon;
            return (
              <div key={item.label} className="rounded-xl border border-line/70 bg-paper/50 p-3">
                <Icon className="h-3.5 w-3.5 text-faint" />
                <p className="mt-1.5 text-[10px] text-faint">{item.label}</p>
                <p className="font-mono text-sm text-ink">{item.value}</p>
              </div>
            );
          })}
        </div>

        {healthError ? <p className="mt-3 text-xs text-danger">{healthError}</p> : null}

        <div className="mt-5 space-y-2 border-t border-line/60 pt-4 text-xs">
          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5 text-muted"><Database className="h-3.5 w-3.5" /> {zh.settings.db}</span>
            <Badge tone={health?.db === "ok" ? "success" : "danger"}>{health?.db ?? "未知"}</Badge>
          </div>
          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5 text-muted"><KeyRound className="h-3.5 w-3.5" /> {zh.settings.llmReady}</span>
            {health?.llm_configured ? (
              <Badge tone="success" dot>已配置</Badge>
            ) : (
              <Badge tone="warning" dot>未配置</Badge>
            )}
          </div>
          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5 text-muted"><ShieldCheck className="h-3.5 w-3.5" /> 访问令牌</span>
            {token ? <Badge tone="success" dot>已设置</Badge> : <Badge tone="neutral">回环访问</Badge>}
          </div>
        </div>
      </section>

      <aside className="space-y-3">
        <section className="surface-card p-4">
          <h3 className="font-display text-sm font-semibold text-ink">安全与隐私</h3>
          <ul className="mt-2 space-y-2 text-[11px] leading-5 text-muted">
            <li className="flex gap-2"><ShieldCheck className="mt-0.5 h-3.5 w-3.5 shrink-0 text-success" /> 数据保存在本机 SQLite，不经过第三方服务器。</li>
            <li className="flex gap-2"><KeyRound className="mt-0.5 h-3.5 w-3.5 shrink-0 text-success" /> API 密钥仅通过环境变量读取，界面只显示状态。</li>
            <li className="flex gap-2"><HardDrive className="mt-0.5 h-3.5 w-3.5 shrink-0 text-success" /> 删除内容进入回收站，永久清除前会二次确认。</li>
          </ul>
        </section>

        <section className="surface-card p-4">
          <h3 className="font-display text-sm font-semibold text-ink">运行环境</h3>
          <dl className="mt-2 space-y-1.5 text-[11px]">
            <div className="flex justify-between gap-2">
              <dt className="text-faint">绑定地址</dt>
              <dd className="font-mono text-muted">{config?.bind ?? "127.0.0.1:8080"}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt className="text-faint">模型</dt>
              <dd className="truncate text-right text-muted">{config?.llm.model ?? "—"}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt className="text-faint">沙盒并发</dt>
              <dd className="font-mono text-muted">{config?.sandbox.max_concurrent ?? "—"}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt className="text-faint">回收站保留</dt>
              <dd className="font-mono text-muted">{config ? `${config.recycle.retention_days} 天` : "—"}</dd>
            </div>
          </dl>
        </section>

        <section className="rounded-xl border border-line/70 bg-paper/50 p-4 text-[10px] leading-5 text-faint">
          <p className="flex items-center gap-1.5"><XCircle className="h-3 w-3" /> 已知限制：前端不内置 Markdown 富文本编辑器与虚拟滚动；500 条事件环形缓冲足够单次推演回看。</p>
        </section>
      </aside>
    </div>
  );
}
