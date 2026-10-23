import type { Priority } from "./types";

export const PRIORITIES: { value: Priority; label: string }[] = [
  { value: "high", label: "重要" },
  { value: "normal", label: "普通" },
  { value: "low", label: "可缓" },
];

export function priorityRank(priority: Priority) {
  return priority === "high" ? 0 : priority === "normal" ? 1 : 2;
}
