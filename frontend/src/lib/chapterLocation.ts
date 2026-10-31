type VolumeLike = { id: string; title: string };
type ChapterLike = { id: string; title: string };

export type ChapterLocation = {
  volumeTitle: string;
  chapterTitle: string;
};

/**
 * 在已加载的大纲树里定位会话绑定的章节。
 * 传入的树可能属于其它小说或尚未加载，找不到时返回 null 交给调用方降级。
 */
export function findChapterLocation(
  volumes: ReadonlyArray<VolumeLike>,
  chaptersByVolume: Readonly<Record<string, ReadonlyArray<ChapterLike>>>,
  chapterId: string | null | undefined,
): ChapterLocation | null {
  if (!chapterId) return null;
  for (const volume of volumes) {
    const chapter = chaptersByVolume[volume.id]?.find((item) => item.id === chapterId);
    if (!chapter) continue;
    return {
      volumeTitle: volume.title || "未命名卷",
      chapterTitle: chapter.title || "未命名章节",
    };
  }
  return null;
}

export function chapterBindingLabel(location: ChapterLocation | null): string {
  return location ? `章节：${location.volumeTitle} / ${location.chapterTitle}` : "章节已删除";
}
