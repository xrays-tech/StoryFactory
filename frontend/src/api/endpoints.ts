import type {
  ChapterDto,
  CreateChapterReq,
  CreateCharacterReq,
  CreateItemReq,
  CreateNovelReq,
  CreateSessionReq,
  CreateVolumeReq,
  CharacterDto,
  ItemDto,
  MoveReq,
  NovelDto,
  PutRelationsReq,
  RelationDto,
  RestoreReq,
  UpdateChapterReq,
  UpdateCharacterReq,
  UpdateItemReq,
  UpdateNovelReq,
  UpdateVolumeReq,
  VolumeDto,
} from "../types/domain";
import type {
  AppConfig,
  ConfigPatch,
  HealthReport,
  ImportReport,
  NovelListQuery,
  RecycleItem,
  SearchItem,
  UsageReport,
  Wire,
  WirePage,
} from "../types/api";
import { ApiError } from "../lib/errors";
import {
  apiUrl,
  authHeaders,
  buildQuery,
  newIdempotencyKey,
  newRequestId,
  request,
  requestBlob,
  uploadFile,
  withTokenInQuery,
  type QueryValue,
} from "./client";

type Session = import("../types/domain").SessionSnapshot;
type ControlReq = import("../types/domain").ControlReq;
type InputReq = import("../types/domain").InputReq;
type SessionStatus = import("../types/domain").SessionStatus;
type RecycleEntity = import("../types/domain").RecycleEntity;

export const paths = {
  healthz: "/api/healthz",
  health: "/api/health",
  novels: "/novels",
  novel: (id: string) => `/novels/${id}`,
  characters: (nid: string) => `/novels/${nid}/characters`,
  character: (nid: string, cid: string) => `/novels/${nid}/characters/${cid}`,
  relations: (nid: string) => `/novels/${nid}/relations`,
  items: (nid: string) => `/novels/${nid}/items`,
  item: (nid: string, iid: string) => `/novels/${nid}/items/${iid}`,
  volumes: (nid: string) => `/novels/${nid}/volumes`,
  volume: (nid: string, vid: string) => `/novels/${nid}/volumes/${vid}`,
  moveVolume: (nid: string, vid: string) => `/novels/${nid}/volumes/${vid}/move`,
  chapters: (nid: string, vid: string) => `/novels/${nid}/volumes/${vid}/chapters`,
  chapter: (nid: string, cid: string) => `/novels/${nid}/chapters/${cid}`,
  novelSessions: (nid: string) => `/novels/${nid}/sandbox-sessions`,
  chapterSessions: (cid: string) => `/chapters/${cid}/sandbox-sessions`,
  session: (sid: string) => `/sandbox-sessions/${sid}`,
  control: (sid: string) => `/sandbox-sessions/${sid}/control`,
  input: (sid: string) => `/sandbox-sessions/${sid}/input`,
  events: (sid: string) => `/sandbox-sessions/${sid}/events`,
  exportNovel: (nid: string) => `/novels/${nid}/export`,
  importPackage: "/import",
  config: "/config",
  usage: "/usage",
  search: "/search",
  recycle: "/recycle-bin",
  restore: "/recycle-bin/restore",
  purge: (entity: RecycleEntity, id: string) => `/recycle-bin/${entity}/${id}`,
  clientErrors: "/client-errors",
};

export async function listNovels(query: NovelListQuery = {}): Promise<WirePage<NovelDto>> {
  return request<WirePage<NovelDto>>("GET", paths.novels, {
    query: {
      q: query.q,
      status: query.status || undefined,
      page: query.page,
      page_size: query.pageSize,
    },
  });
}

export async function createNovel(req: Wire<CreateNovelReq>): Promise<Wire<NovelDto>> {
  return request<Wire<NovelDto>>("POST", paths.novels, {
    body: req,
    idempotencyKey: newIdempotencyKey("novel"),
  });
}

export async function getNovel(id: string): Promise<Wire<NovelDto>> {
  return request<Wire<NovelDto>>("GET", paths.novel(id));
}

export async function updateNovel(
  id: string,
  req: Partial<Wire<UpdateNovelReq>>,
): Promise<Wire<NovelDto>> {
  return request<Wire<NovelDto>>("PATCH", paths.novel(id), { body: req });
}

export async function deleteNovel(id: string): Promise<void> {
  await request<void>("DELETE", paths.novel(id));
}

export async function listCharacters(
  nid: string,
  query: { page?: number; pageSize?: number } = {},
): Promise<WirePage<CharacterDto>> {
  return request<WirePage<CharacterDto>>("GET", paths.characters(nid), {
    query: { page: query.page, page_size: query.pageSize },
  });
}

export async function createCharacter(
  nid: string,
  req: Wire<CreateCharacterReq>,
): Promise<Wire<CharacterDto>> {
  return request<Wire<CharacterDto>>("POST", paths.characters(nid), { body: req });
}

export async function updateCharacter(
  nid: string,
  cid: string,
  req: Partial<Wire<UpdateCharacterReq>>,
): Promise<Wire<CharacterDto>> {
  return request<Wire<CharacterDto>>("PATCH", paths.character(nid, cid), { body: req });
}

export async function deleteCharacter(nid: string, cid: string): Promise<void> {
  await request<void>("DELETE", paths.character(nid, cid));
}

export async function getRelations(nid: string): Promise<Array<Wire<RelationDto>>> {
  return request<Array<Wire<RelationDto>>>("GET", paths.relations(nid));
}

export async function putRelations(
  nid: string,
  req: Wire<PutRelationsReq>,
): Promise<Array<Wire<RelationDto>>> {
  return request<Array<Wire<RelationDto>>>("PUT", paths.relations(nid), { body: req });
}

export async function listItems(
  nid: string,
  query: { page?: number; pageSize?: number } = {},
): Promise<WirePage<ItemDto>> {
  return request<WirePage<ItemDto>>("GET", paths.items(nid), {
    query: { page: query.page, page_size: query.pageSize },
  });
}

export async function createItem(nid: string, req: Wire<CreateItemReq>): Promise<Wire<ItemDto>> {
  return request<Wire<ItemDto>>("POST", paths.items(nid), { body: req });
}

export async function updateItem(
  nid: string,
  iid: string,
  req: Partial<Wire<UpdateItemReq>>,
): Promise<Wire<ItemDto>> {
  return request<Wire<ItemDto>>("PATCH", paths.item(nid, iid), { body: req });
}

export async function deleteItem(nid: string, iid: string): Promise<void> {
  await request<void>("DELETE", paths.item(nid, iid));
}

export async function listVolumes(nid: string): Promise<Array<Wire<VolumeDto>>> {
  return request<Array<Wire<VolumeDto>>>("GET", paths.volumes(nid));
}

export async function createVolume(
  nid: string,
  req: Wire<CreateVolumeReq>,
): Promise<Wire<VolumeDto>> {
  return request<Wire<VolumeDto>>("POST", paths.volumes(nid), { body: req });
}

export async function updateVolume(
  nid: string,
  vid: string,
  req: Partial<Wire<UpdateVolumeReq>>,
): Promise<Wire<VolumeDto>> {
  return request<Wire<VolumeDto>>("PATCH", paths.volume(nid, vid), { body: req });
}

export async function deleteVolume(nid: string, vid: string): Promise<void> {
  await request<void>("DELETE", paths.volume(nid, vid));
}

export async function moveVolume(
  nid: string,
  vid: string,
  req: Wire<MoveReq>,
): Promise<Array<Wire<VolumeDto>>> {
  return request<Array<Wire<VolumeDto>>>("PATCH", paths.moveVolume(nid, vid), { body: req });
}

export async function listChapters(
  nid: string,
  vid: string,
  query: { page?: number; pageSize?: number } = {},
): Promise<WirePage<ChapterDto>> {
  return request<WirePage<ChapterDto>>("GET", paths.chapters(nid, vid), {
    query: { page: query.page, page_size: query.pageSize },
  });
}

export async function getChapter(nid: string, cid: string): Promise<Wire<ChapterDto>> {
  return request<Wire<ChapterDto>>("GET", paths.chapter(nid, cid));
}

export async function createChapter(
  nid: string,
  vid: string,
  req: Wire<CreateChapterReq>,
): Promise<Wire<ChapterDto>> {
  return request<Wire<ChapterDto>>("POST", paths.chapters(nid, vid), { body: req });
}

export async function updateChapter(
  nid: string,
  cid: string,
  req: Partial<Wire<UpdateChapterReq>>,
): Promise<Wire<ChapterDto>> {
  return request<Wire<ChapterDto>>("PATCH", paths.chapter(nid, cid), { body: req });
}

export async function deleteChapter(nid: string, cid: string): Promise<void> {
  await request<void>("DELETE", paths.chapter(nid, cid));
}

export async function listSessions(
  nid: string,
  query: { status?: SessionStatus | ""; page?: number; pageSize?: number } = {},
): Promise<WirePage<Session>> {
  return request<WirePage<Session>>("GET", paths.novelSessions(nid), {
    query: { status: query.status || undefined, page: query.page, page_size: query.pageSize },
  });
}

export async function createSession(
  novelId: string,
  chapterId: string | null,
  req: Wire<CreateSessionReq>,
  idempotencyKey = newIdempotencyKey("session"),
): Promise<Wire<Session>> {
  const path = chapterId ? paths.chapterSessions(chapterId) : paths.novelSessions(novelId);
  return request<Wire<Session>>("POST", path, { body: req, idempotencyKey });
}

export async function getSession(sid: string): Promise<Wire<Session>> {
  return request<Wire<Session>>("GET", paths.session(sid));
}

export async function controlSession(
  sid: string,
  req: Wire<ControlReq>,
): Promise<Wire<Session>> {
  return request<Wire<Session>>("POST", paths.control(sid), { body: req });
}

export async function inputSession(sid: string, req: Wire<InputReq>): Promise<Wire<Session>> {
  return request<Wire<Session>>("POST", paths.input(sid), { body: req });
}

export function eventsUrl(sid: string, afterSeq?: number, extra?: Record<string, QueryValue>): string {
  const query = buildQuery({ after_seq: afterSeq, ...extra });
  return withTokenInQuery(`${apiUrl(paths.events(sid))}${query}`);
}

export async function exportNovel(
  nid: string,
  options: { includeSessions?: boolean; includeDeleted?: boolean } = {},
): Promise<{ blob: Blob; filename: string | null }> {
  return requestBlob("GET", paths.exportNovel(nid), {
    query: {
      include_sessions: options.includeSessions ?? true,
      include_deleted: options.includeDeleted ?? false,
    },
  });
}

export async function importPackage(file: File, idempotencyKey = newIdempotencyKey("import")): Promise<ImportReport> {
  return uploadFile<ImportReport>(paths.importPackage, file, { idempotencyKey });
}

export async function getConfig(): Promise<AppConfig> {
  return request<AppConfig>("GET", paths.config);
}

export async function patchConfig(patch: ConfigPatch): Promise<AppConfig> {
  return request<AppConfig>("PATCH", paths.config, { body: patch });
}

export async function getUsage(month?: string): Promise<UsageReport> {
  return request<UsageReport>("GET", paths.usage, { query: { month } });
}

export async function search(
  q: string,
  options: { type?: string; novelId?: string; page?: number; pageSize?: number } = {},
): Promise<WirePage<SearchItem>> {
  return request<WirePage<SearchItem>>("GET", paths.search, {
    query: {
      q,
      type: options.type,
      novel_id: options.novelId,
      page: options.page,
      page_size: options.pageSize,
    },
  });
}

export async function listRecycle(
  query: { type?: RecycleEntity | ""; novelId?: string; page?: number; pageSize?: number } = {},
): Promise<WirePage<RecycleItem>> {
  return request<WirePage<RecycleItem>>("GET", paths.recycle, {
    query: {
      type: query.type || undefined,
      novel_id: query.novelId,
      page: query.page,
      page_size: query.pageSize,
    },
  });
}

export async function restoreRecycle(req: Wire<RestoreReq>): Promise<string[]> {
  return request<string[]>("POST", paths.restore, { body: req });
}

export async function purgeRecycle(entity: RecycleEntity, id: string): Promise<void> {
  await request<void>("DELETE", paths.purge(entity, id));
}

export async function getHealth(): Promise<HealthReport> {
  const response = await fetch("/api/health", {
    headers: authHeaders({ "X-Request-Id": newRequestId() }),
  });
  if (!response.ok) {
    throw new ApiError({
      status: response.status,
      message: `健康检查失败 (${response.status})`,
    });
  }
  return (await response.json()) as HealthReport;
}
