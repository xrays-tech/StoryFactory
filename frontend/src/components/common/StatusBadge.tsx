import type { ChapterStatus, RoleType, SessionStatus } from "../../types/domain";
import { chapterStatusLabel, roleLabel, sessionStatusLabel } from "../../copy/zh";
import { Badge, type BadgeTone } from "./Badge";

const sessionTone: Record<SessionStatus, BadgeTone> = {
  created: "neutral",
  running: "accent",
  paused: "warning",
  waiting_input: "info",
  finalizing: "info",
  completed: "success",
  failed: "danger",
  interrupted: "warning",
  aborted: "muted",
};

export function SessionStatusBadge({
  status,
  className,
}: {
  status: SessionStatus;
  className?: string;
}) {
  return (
    <Badge
      tone={sessionTone[status]}
      dot
      pulse={status === "running" || status === "finalizing"}
      className={className}
    >
      {sessionStatusLabel[status]}
    </Badge>
  );
}

const chapterTone: Record<ChapterStatus, BadgeTone> = {
  draft: "neutral",
  outlined: "info",
  completed: "success",
  archived: "muted",
};

export function ChapterStatusBadge({ status }: { status: ChapterStatus }) {
  return <Badge tone={chapterTone[status]}>{chapterStatusLabel[status]}</Badge>;
}

const roleTone: Record<RoleType, BadgeTone> = {
  protagonist: "accent",
  deuteragonist: "info",
  antagonist: "danger",
  support: "neutral",
};

export function RoleBadge({ role }: { role: RoleType }) {
  return <Badge tone={roleTone[role]}>{roleLabel[role]}</Badge>;
}
