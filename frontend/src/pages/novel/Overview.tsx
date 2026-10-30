import { Link, useLoaderData, useParams } from "react-router-dom";
import type { LoaderFunctionArgs } from "react-router-dom";
import type { ReactNode } from "react";
import {
  ArrowRight,
  BookOpenText,
  Boxes,
  HeartHandshake,
  ListTree,
  Package,
  Sparkles,
  Users,
} from "lucide-react";
import * as api from "../../api/endpoints";
import type { Session } from "../../stores/sandboxStore";
import type { Chapter, Volume } from "../../stores/outlineStore";
import { useNovelStore } from "../../stores/novelStore";
import { zh } from "../../copy/zh";
import { formatDateTime, truncate } from "../../lib/format";
import { SessionStatusBadge } from "../../components/common/StatusBadge";
import { Skeleton } from "../../components/common/Skeleton";
import { cn } from "../../lib/cn";

export type OverviewData = {
  characters: number;
  volumes: Array<{ volume: Volume; chapters: number }>;
  chapters: number;
  sessions: number;
  recentSessions: Session[];
  error?: string;
};

export async function overviewLoader({ params }: LoaderFunctionArgs): Promise<OverviewData> {
  const novelId = params.novelId;
  if (!novelId) {
    return { characters: 0, volumes: [], chapters: 0, sessions: 0, recentSessions: [] };
  }
  try {
    const [characters, volumes, sessions] = await Promise.all([
      api.listCharacters(novelId, { page: 1, pageSize: 100 }),
      api.listVolumes(novelId),
      api.listSessions(novelId, { page: 1, pageSize: 5 }),
    ]);
    const chapterPages = await Promise.all(
      volumes.map((volume) => api.listChapters(novelId, volume.id, { page: 1, pageSize: 100 })),
    );
    const volumeRows = volumes.map((volume, index) => ({
      volume,
      chapters: chapterPages[index].total,
    }));
    return {
      characters: characters.total,
      volumes: volumeRows,
      chapters: chapterPages.reduce((sum, page) => sum + page.total, 0),
      sessions: sessions.total,
      recentSessions: sessions.items,
    };
  } catch (error) {
    return {
      characters: 0,
      volumes: [],
      chapters: 0,
      sessions: 0,
      recentSessions: [],
      error: error instanceof Error ? error.message : "概览读取失败",
    };
  }
}

function StatCard({
  icon,
  label,
  value,
  to,
}: {
  icon: ReactNode;
  label: string;
  value: number;
  to: string;
}) {
  return (
    <Link
      to={to}
      className="group surface-card flex items-center gap-4 p-4 transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/30 hover:shadow-float"
    >
      <span className="flex h-10 w-10 items-center justify-center rounded-xl bg-accent-soft/70 text-accent-strong transition-colors group-hover:bg-accent group-hover:text-white dark:group-hover:text-[#201612]">
        {icon}
      </span>
      <span className="min-w-0">
        <span className="block font-display text-2xl font-semibold leading-none text-ink">{value}</span>
        <span className="mt-1 block text-xs text-muted">{label}</span>
      </span>
      <ArrowRight className="ml-auto h-4 w-4 text-faint opacity-0 transition-opacity group-hover:opacity-100" />
    </Link>
  );
}

function SectionCard({
  title,
  children,
  action,
  className,
}: {
  title: string;
  children: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("surface-card p-5", className)}>
      <div className="mb-3 flex items-center justify-between gap-2">
        <h2 className="font-display text-base font-semibold text-ink">{title}</h2>
        {action}
      </div>
      {children}
    </section>
  );
}

export function NovelOverview() {
  const { novelId } = useParams<{ novelId: string }>();
  const data = useLoaderData() as OverviewData;
  const novel = useNovelStore((s) => s.current);
  if (!novelId || !novel) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-6 w-40" />
        <Skeleton className="h-32 w-full" />
      </div>
    );
  }
  const base = `/novel/${novelId}`;

  return (
    <div className="space-y-5">
      {data.error ? (
        <p className="rounded-xl border border-warning/30 bg-warning-soft/50 px-3 py-2 text-sm text-warning">
          概览数据读取不完整：{data.error}
        </p>
      ) : null}

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard icon={<Users className="h-4 w-4" />} label={zh.novel.characters} value={data.characters} to={`${base}/characters`} />
        <StatCard icon={<ListTree className="h-4 w-4" />} label="卷" value={data.volumes.length} to={`${base}/outline`} />
        <StatCard icon={<BookOpenText className="h-4 w-4" />} label="章节" value={data.chapters} to={`${base}/outline`} />
        <StatCard icon={<Boxes className="h-4 w-4" />} label="沙盒会话" value={data.sessions} to={`${base}/sandbox`} />
      </div>

      <div className="grid gap-5 lg:grid-cols-[1.35fr_1fr]">
        <SectionCard
          title="故事骨架"
          action={
            <Link to={`${base}/outline`} className="text-xs font-medium text-accent-strong hover:underline">
              打开大纲
            </Link>
          }
        >
          {data.volumes.length === 0 ? (
            <div className="rounded-xl border border-dashed border-line/90 px-4 py-8 text-center">
              <p className="text-sm text-muted">还没有卷。先给故事搭一个骨架。</p>
              <Link to={`${base}/outline`} className="mt-2 inline-flex text-xs font-medium text-accent-strong hover:underline">
                创建第一卷
              </Link>
            </div>
          ) : (
            <ol className="space-y-2">
              {data.volumes.map(({ volume, chapters }) => (
                <li key={volume.id}>
                  <Link
                    to={`${base}/outline`}
                    className="group flex items-center gap-3 rounded-xl border border-line/60 bg-paper/40 px-3 py-2.5 transition-colors hover:border-accent/30 hover:bg-raised"
                  >
                    <span className="font-mono text-[11px] text-faint">
                      {String(volume.volume_index).padStart(2, "0")}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium text-ink">{volume.title || "未命名卷"}</span>
                      <span className="mt-0.5 block truncate text-[11px] text-faint">
                        {volume.summary || "暂无卷摘要"}
                      </span>
                    </span>
                    <span className="shrink-0 text-[11px] text-muted">{chapters} 章</span>
                  </Link>
                </li>
              ))}
            </ol>
          )}
        </SectionCard>

        <SectionCard
          title="最近推演"
          action={
            <Link to={`${base}/sandbox`} className="text-xs font-medium text-accent-strong hover:underline">
              全部会话
            </Link>
          }
        >
          {data.recentSessions.length === 0 ? (
            <div className="rounded-xl border border-dashed border-line/90 px-4 py-8 text-center">
              <p className="text-sm text-muted">还没有沙盒会话。</p>
              <Link to={`${base}/sandbox`} className="mt-2 inline-flex text-xs font-medium text-accent-strong hover:underline">
                发起第一次推演
              </Link>
            </div>
          ) : (
            <ul className="space-y-2">
              {data.recentSessions.map((session) => (
                <li key={session.id}>
                  <Link
                    to={`/sandbox/${session.id}`}
                    className="flex items-center gap-2.5 rounded-xl border border-line/60 bg-paper/40 px-3 py-2.5 transition-colors hover:border-accent/30 hover:bg-raised"
                  >
                    <SessionStatusBadge status={session.status} />
                    <span className="min-w-0 flex-1 truncate text-xs text-muted">
                      {session.actor_names.map((actor) => actor.name).join("、") || "无角色"}
                    </span>
                    <span className="shrink-0 text-[11px] text-faint">
                      {session.current_step}/{session.step_limit}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </SectionCard>
      </div>

      <div className="grid gap-5 lg:grid-cols-3">
        <SectionCard title={zh.novel.worldView}>
          <p className="whitespace-pre-wrap text-sm leading-6 text-muted">
            {truncate(novel.world_view || "还没有世界观设定。", 480)}
          </p>
        </SectionCard>
        <SectionCard title={zh.novel.background}>
          <p className="whitespace-pre-wrap text-sm leading-6 text-muted">
            {truncate(novel.background || "还没有背景设定。", 480)}
          </p>
        </SectionCard>
        <SectionCard title={zh.novel.summary}>
          <p className="whitespace-pre-wrap text-sm leading-6 text-muted">
            {truncate(novel.summary || "还没有故事摘要。", 480)}
          </p>
        </SectionCard>
      </div>

      <section className="surface-card p-5">
        <div className="flex items-center gap-2">
          <Sparkles className="h-4 w-4 text-accent" />
          <h2 className="font-display text-base font-semibold text-ink">从这里继续</h2>
        </div>
        <div className="mt-3 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
          {[
            { to: `${base}/characters`, label: "整理角色", hint: "人物卡与目标", icon: Users },
            { to: `${base}/relations`, label: "调整羁绊", hint: "亲密度与信任", icon: HeartHandshake },
            { to: `${base}/items`, label: "补充物品", hint: "线索与信物", icon: Package },
            { to: `${base}/sandbox`, label: "发起推演", hint: "让角色走几步", icon: Boxes },
          ].map((item) => {
            const Icon = item.icon;
            return (
              <Link
                key={item.to}
                to={item.to}
                className="group flex items-center gap-3 rounded-xl border border-line/60 bg-paper/40 px-3 py-2.5 transition-colors hover:border-accent/30 hover:bg-raised"
              >
                <Icon className="h-4 w-4 text-accent" />
                <span>
                  <span className="block text-sm font-medium text-ink">{item.label}</span>
                  <span className="block text-[11px] text-faint">{item.hint}</span>
                </span>
                <ArrowRight className="ml-auto h-3.5 w-3.5 text-faint opacity-0 transition-opacity group-hover:opacity-100" />
              </Link>
            );
          })}
        </div>
        <p className="mt-3 text-[11px] text-faint">
          最近更新：{formatDateTime(novel.updated_at)}
        </p>
      </section>
    </div>
  );
}
