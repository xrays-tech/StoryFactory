export type DraftPov = "第一人称" | "第三人称";

export const STYLE_LIMITS = {
  name: 40,
  sample: 6000,
  instruction: 1000,
} as const;

/** 小说级文风档案中与生成请求相关的字段。 */
export type StyleProfile = {
  style_name: string | null;
  style_sample: string | null;
  style_instruction: string | null;
  pov: string | null;
};

/** 生成弹窗里的临时文风表单。 */
export type StyleForm = {
  style_name: string;
  style_sample: string;
  instruction: string;
  pov: DraftPov;
};

export type StylePayload = {
  style_name: string | null;
  style_sample: string | null;
  instruction: string | null;
  pov: DraftPov | null;
};

/**
 * 字段与档案一致时返回 null，让后端回退到小说档案；
 * 用户改过的字段才提交当前值。表单留空而档案非空同样视为沿用档案。
 */
export function resolveStylePayload(
  profile: StyleProfile | null | undefined,
  form: StyleForm,
): StylePayload {
  return {
    style_name: resolveText(profile?.style_name, form.style_name),
    style_sample: resolveText(profile?.style_sample, form.style_sample),
    instruction: resolveText(profile?.style_instruction, form.instruction),
    pov: resolvePov(profile?.pov, form.pov),
  };
}

function resolveText(profileValue: string | null | undefined, formValue: string): string | null {
  const profile = profileValue?.trim() ?? "";
  const value = formValue.trim();
  const followsProfile = value === "" && profile !== "";
  if (value === profile || followsProfile) return null;
  return value || null;
}

function resolvePov(profileValue: string | null | undefined, formValue: DraftPov): DraftPov | null {
  const profile = asDraftPov(profileValue);
  // 档案没写人称时，第三人称就是后端默认值，无需显式提交。
  if (!profile) return formValue === "第三人称" ? null : formValue;
  return profile === formValue ? null : formValue;
}

function asDraftPov(value: string | null | undefined): DraftPov | null {
  if (value === "第一人称" || value === "第三人称") return value;
  return null;
}
