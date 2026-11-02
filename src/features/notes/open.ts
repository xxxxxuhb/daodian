import type { InjectionKey, Ref } from "vue";

export interface LinkedTarget {
  kind: "todo" | "plan" | "stage";
  id: string;
  planId?: string;
  title: string;
}

export const openLinkedKey: InjectionKey<(target: LinkedTarget) => void> = Symbol("openLinked");
export const openNoteKey: InjectionKey<(id: string) => void> = Symbol("openNote");
export const mediaDirKey: InjectionKey<Ref<string>> = Symbol("mediaDir");
