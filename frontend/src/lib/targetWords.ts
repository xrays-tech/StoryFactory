export const DEFAULT_CHAPTER_TARGET_WORDS = 3500;
export const CHAPTER_TARGET_MIN = 300;
export const CHAPTER_TARGET_MAX = 20000;
export const DRAFT_TARGET_MIN = 500;
export const DRAFT_TARGET_MAX = 8000;

/**
 * 把表单输入或接口数字解析成 [min, max] 内的整数。
 * 空白、非数字、越界一律返回 null，让调用方决定错误提示。
 */
export function parseTargetWords(
  input: string | number | null | undefined,
  min: number,
  max: number,
): number | null {
  if (input === null || input === undefined) return null;
  if (typeof input === "string" && input.trim() === "") return null;
  const value = Number(input);
  if (!Number.isInteger(value) || value < min || value > max) return null;
  return value;
}

/** 章节记录里的目标字数；缺失或越界时回退到 3500。 */
export function chapterTargetWords(value: number | null | undefined): number {
  return (
    parseTargetWords(value, CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX) ??
    DEFAULT_CHAPTER_TARGET_WORDS
  );
}

/** 正文生成弹窗的初始值：章节目标字数，收敛到草稿接口允许的 500–8000。 */
export function draftTargetWordsForChapter(value: number | null | undefined): number {
  return Math.min(DRAFT_TARGET_MAX, Math.max(DRAFT_TARGET_MIN, chapterTargetWords(value)));
}
