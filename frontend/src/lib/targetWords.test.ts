import { describe, expect, it } from "vitest";
import {
  CHAPTER_TARGET_MAX,
  CHAPTER_TARGET_MIN,
  DEFAULT_CHAPTER_TARGET_WORDS,
  DRAFT_TARGET_MAX,
  DRAFT_TARGET_MIN,
  chapterTargetWords,
  draftTargetWordsForChapter,
  parseTargetWords,
} from "./targetWords";

describe("parseTargetWords", () => {
  it("accepts integers inside the range", () => {
    expect(parseTargetWords("3500", CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBe(3500);
    expect(parseTargetWords(300, CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBe(300);
    expect(parseTargetWords(20000, CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBe(20000);
  });

  it("rejects blank, fractional, non-numeric and out-of-range input", () => {
    expect(parseTargetWords("", CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
    expect(parseTargetWords("   ", CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
    expect(parseTargetWords("3500.5", CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
    expect(parseTargetWords("abc", CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
    expect(parseTargetWords("299", CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
    expect(parseTargetWords("20001", CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
    expect(parseTargetWords(null, CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
    expect(parseTargetWords(undefined, CHAPTER_TARGET_MIN, CHAPTER_TARGET_MAX)).toBeNull();
  });
});

describe("chapterTargetWords", () => {
  it("returns a valid stored target", () => {
    expect(chapterTargetWords(4200)).toBe(4200);
  });

  it("falls back to 3500 when the value is missing or invalid", () => {
    expect(chapterTargetWords(null)).toBe(DEFAULT_CHAPTER_TARGET_WORDS);
    expect(chapterTargetWords(undefined)).toBe(DEFAULT_CHAPTER_TARGET_WORDS);
    expect(chapterTargetWords(0)).toBe(DEFAULT_CHAPTER_TARGET_WORDS);
    expect(chapterTargetWords(20001)).toBe(DEFAULT_CHAPTER_TARGET_WORDS);
  });
});

describe("draftTargetWordsForChapter", () => {
  it("uses the chapter target when it fits the draft range", () => {
    expect(draftTargetWordsForChapter(4200)).toBe(4200);
  });

  it("favours the chapter target when possible", () => {
    expect(draftTargetWordsForChapter(DEFAULT_CHAPTER_TARGET_WORDS)).toBe(
      DEFAULT_CHAPTER_TARGET_WORDS,
    );
  });

  it("clamps the chapter target into the draft range", () => {
    expect(draftTargetWordsForChapter(20000)).toBe(DRAFT_TARGET_MAX);
    expect(draftTargetWordsForChapter(300)).toBe(DRAFT_TARGET_MIN);
  });

  it("falls back to 3500 when the chapter target is unavailable", () => {
    expect(draftTargetWordsForChapter(null)).toBe(DEFAULT_CHAPTER_TARGET_WORDS);
  });
});
