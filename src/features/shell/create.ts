import type { InjectionKey, Ref } from "vue";

export const createTickKey: InjectionKey<Ref<number>> = Symbol("createTick");
