export const DEFAULT_GROUP = "默认";

export const SETTING_TYPES = ["String", "Int", "Float", "Bool"] as const;

export type SettingType = (typeof SETTING_TYPES)[number];
