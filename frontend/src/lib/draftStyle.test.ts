import { describe, expect, it } from "vitest";
import { resolveStylePayload, type StyleForm, type StyleProfile } from "./draftStyle";

const emptyForm: StyleForm = {
  style_name: "",
  style_sample: "",
  instruction: "",
  pov: "第三人称",
};

const profile: StyleProfile = {
  style_name: "古龙式短句",
  style_sample: "风很冷。刀更冷。",
  style_instruction: "多用短句，少用形容词。",
  pov: "第一人称",
};

describe("resolveStylePayload", () => {
  it("returns all null when both profile and form are empty", () => {
    expect(resolveStylePayload(null, emptyForm)).toEqual({
      style_name: null,
      style_sample: null,
      instruction: null,
      pov: null,
    });
  });

  it("returns all null when the form follows the profile exactly", () => {
    expect(
      resolveStylePayload(profile, {
        style_name: "  古龙式短句 ",
        style_sample: "风很冷。刀更冷。",
        instruction: "多用短句，少用形容词。",
        pov: "第一人称",
      }),
    ).toEqual({
      style_name: null,
      style_sample: null,
      instruction: null,
      pov: null,
    });
  });

  it("only submits the fields the user changed", () => {
    expect(
      resolveStylePayload(profile, {
        style_name: "古龙式短句",
        style_sample: "雨很大。灯很暗。",
        instruction: "多用短句，少用形容词。",
        pov: "第三人称",
      }),
    ).toEqual({
      style_name: null,
      style_sample: "雨很大。灯很暗。",
      instruction: null,
      pov: "第三人称",
    });
  });

  it("returns null when the user clears a profile field", () => {
    expect(
      resolveStylePayload(profile, {
        ...emptyForm,
        style_name: "古龙式短句",
        instruction: "   ",
        pov: "第一人称",
      }),
    ).toEqual({
      style_name: null,
      style_sample: null,
      instruction: null,
      pov: null,
    });
  });
});
