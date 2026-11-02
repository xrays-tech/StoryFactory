import type { CreateSessionReq, DirectorMode, ScriptedPlan } from "../types/domain";
import type { Wire } from "../types/api";

export type CreateSessionDraft = {
  actorIds: string[];
  mode: DirectorMode;
  scenePrompt: string;
  stepLimit: number;
  directorConfig: Wire<ScriptedPlan> | null;
  userDirective: string | null;
  autostart: boolean;
  chapterId: string | null;
  /** 绑定章节且为 true 时，推演收束后自动生成该章正文。 */
  autoProse: boolean;
};

/**
 * 会话创建请求体的唯一构造入口：路由里的 chapterId 决定接口路径，
 * 请求体里的 chapter_id 显式表达绑定关系（空值表示整本书级推演）。
 */
export function buildCreateSessionPayload(draft: CreateSessionDraft): Wire<CreateSessionReq> {
  const chapterId = draft.chapterId?.trim() || null;
  return {
    actor_ids: [...draft.actorIds],
    mode: draft.mode,
    scene_prompt: draft.scenePrompt,
    step_limit: draft.stepLimit,
    director_config: draft.directorConfig,
    user_directive: draft.userDirective,
    autostart: draft.autostart,
    chapter_id: chapterId,
    // 未绑定章节时后端没有可写入的正文目标，显式发 null 表示关闭。
    auto_prose: chapterId ? draft.autoProse : null,
  };
}
