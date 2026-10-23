import type { ComputedRef, InjectionKey } from "vue";

export type Tier = "mini" | "standard" | "wide";

export const tierKey: InjectionKey<ComputedRef<Tier>> = Symbol("tier");
