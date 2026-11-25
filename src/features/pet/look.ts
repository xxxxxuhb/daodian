// 珠子的外观清单：id 存进 settings.json，渲染和设置页共用同一份。
// 只做发型与发色；光圈、背翼、材质等其它挂点等以后再说。
export const HAIR_OPTIONS = [
  { id: "none", name: "不戴" },
  { id: "side", name: "侧分" },
  { id: "tails", name: "双尾" },
  { id: "spike", name: "刺猬" },
  { id: "long", name: "长直" },
  { id: "asym", name: "单边长" },
  { id: "buns", name: "双丸子" },
  { id: "pin", name: "发卡" },
] as const;

export type HairId = (typeof HAIR_OPTIONS)[number]["id"];

export const HAIR_COLORS = [
  { id: "ink", name: "墨", value: "#0E1613" },
  { id: "chestnut", name: "栗棕", value: "#4A3226" },
  { id: "wine", name: "酒红", value: "#6E2F38" },
  { id: "flax", name: "亚麻", value: "#C2A16A" },
  { id: "silver", name: "银", value: "#CFD6D4" },
] as const;

export type HairColorId = (typeof HAIR_COLORS)[number]["id"];

// 珠子本体的配色：只换本体/环/光晕，眼睛和语义色（到点琥珀）不跟着变。
export const PET_PALETTES = [
  { id: "celadon", name: "青瓷", body: "#15637B", ring: "rgba(21,99,123,.18)", halo: "rgba(21,99,123,.38)" },
  { id: "clay", name: "陶砂", body: "#B06A4A", ring: "rgba(176,106,74,.22)", halo: "rgba(176,106,74,.40)" },
  { id: "moss", name: "苔绿", body: "#3F7A60", ring: "rgba(63,122,96,.24)", halo: "rgba(63,122,96,.40)" },
  { id: "indigo", name: "靛蓝", body: "#2E4470", ring: "rgba(46,68,112,.24)", halo: "rgba(46,68,112,.42)" },
] as const;

export type PetPaletteId = (typeof PET_PALETTES)[number]["id"];

export function petPalette(value: string | null | undefined) {
  return PET_PALETTES.find((item) => item.id === value) ?? PET_PALETTES[0];
}

export function hairId(value: string | null | undefined): HairId {
  const hit = HAIR_OPTIONS.find((item) => item.id === value);
  return hit ? hit.id : "none";
}

export function hairColor(value: string | null | undefined): string {
  const hit = HAIR_COLORS.find((item) => item.id === value);
  return hit ? hit.value : HAIR_COLORS[0].value;
}
