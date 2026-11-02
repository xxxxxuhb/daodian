import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onMounted, onUnmounted, ref } from "vue";
import { createNote, deleteNote, loadNote, loadNotesIndex, saveNote, setNoteMeta } from "../../api/notes";
import { errorText } from "../../domain/feedback";
import { emptyDoc, type Note, type NoteIndex, type NoteMetaPatch, type NoteNode } from "../../domain/notes";
import { describeDoc, docBytes, MAX_DOC_BYTES } from "./logic";

const notes = ref<NoteIndex[]>([]);
const attachmentsDir = ref("");
const activeId = ref<string | null>(null);
const current = ref<Note | null>(null);
const loading = ref(true);
const loadError = ref("");
const saveState = ref<"saved" | "saving" | "dirty" | "error">("saved");
const saveError = ref("");
const savedAt = ref<number | null>(null);
let dirty = false;
let timer: number | null = null;
let flushing: Promise<void> | null = null;
let listening = false;

function replaceIndex(index: NoteIndex) {
  const next = notes.value.filter((item) => item.id !== index.id);
  next.push(index);
  notes.value = next;
}

function indexOf(note: Note): NoteIndex {
  const described = describeDoc(note.doc);
  return {
    id: note.id,
    title: note.title,
    summary: described.summary,
    text: described.text,
    tags: note.tags,
    dueAt: note.dueAt,
    pinned: note.pinned,
    archived: note.archived,
    reminded: note.reminded,
    thumb: described.thumb ?? null,
    attachments: described.attachments,
    createdAt: note.createdAt,
    updatedAt: note.updatedAt,
  };
}

export function useNotes() {
  async function reload() {
    try {
      const data = await loadNotesIndex();
      const openId = activeId.value;
      const previousUpdated = current.value?.updatedAt;
      notes.value = data.notes;
      attachmentsDir.value = data.attachmentsDir;
      loadError.value = "";
      if (openId && !data.notes.some((item) => item.id === openId)) {
        activeId.value = null;
        current.value = null;
        dirty = false;
      } else if (openId && current.value && !dirty && previousUpdated != null) {
        const fresh = data.notes.find((item) => item.id === openId);
        if (fresh && fresh.updatedAt !== previousUpdated) {
          current.value = await loadNote(openId);
          savedAt.value = current.value.updatedAt;
        }
      }
    } catch (error) {
      loadError.value = errorText(error);
    } finally {
      loading.value = false;
    }
  }

  async function open(id: string) {
    await flush();
    const note = await loadNote(id);
    activeId.value = id;
    current.value = note;
    dirty = false;
    saveState.value = "saved";
    saveError.value = "";
    savedAt.value = note.updatedAt;
  }

  async function create(seed?: string) {
    await flush();
    const note = await createNote(seed);
    replaceIndex(indexOf(note));
    activeId.value = note.id;
    current.value = { ...note, doc: note.doc ?? emptyDoc() };
    dirty = false;
    saveState.value = "saved";
    saveError.value = "";
    savedAt.value = note.updatedAt;
    return note;
  }

  function edit(patch: Partial<Pick<Note, "title" | "doc" | "tags" | "dueAt" | "pinned" | "archived">>) {
    if (!current.value) return;
    current.value = { ...current.value, ...patch };
    const edited = current.value;
    notes.value = notes.value.map((item) =>
      item.id === edited.id
        ? {
            ...item,
            title: edited.title,
            tags: edited.tags,
            dueAt: edited.dueAt,
            pinned: edited.pinned,
            archived: edited.archived,
          }
        : item,
    );
    dirty = true;
    saveState.value = "dirty";
    saveError.value = "";
    if (timer != null) window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      timer = null;
      void flush();
    }, 800);
  }

  async function persist() {
    const snapshot = current.value;
    if (!snapshot || !dirty) return;
    if (docBytes(snapshot.doc) > MAX_DOC_BYTES) {
      saveState.value = "error";
      saveError.value = "正文超过 2MB，先拆成几篇";
      return;
    }
    const described = describeDoc(snapshot.doc);
    dirty = false;
    saveState.value = "saving";
    try {
      const index = await saveNote({
        id: snapshot.id,
        title: snapshot.title,
        doc: snapshot.doc,
        docVersion: snapshot.docVersion || 1,
        tags: snapshot.tags,
        dueAt: snapshot.dueAt ?? null,
        pinned: snapshot.pinned,
        archived: snapshot.archived,
        thumb: described.thumb ?? null,
        attachments: described.attachments,
        text: described.text,
        summary: described.summary,
        baseUpdatedAt: snapshot.updatedAt,
      });
      replaceIndex(index);
      if (current.value?.id === snapshot.id) {
        const local = current.value;
        current.value = {
          ...local,
          ...(dirty
            ? {}
            : {
                title: index.title,
                tags: index.tags,
                dueAt: index.dueAt,
                pinned: index.pinned,
                archived: index.archived,
              }),
          reminded: index.reminded,
          summary: index.summary,
          text: index.text,
          thumb: index.thumb,
          attachments: index.attachments,
          updatedAt: index.updatedAt,
        };
      }
      saveState.value = dirty ? "dirty" : "saved";
      if (!dirty) savedAt.value = index.updatedAt;
      saveError.value = "";
    } catch (error) {
      dirty = true;
      saveState.value = "error";
      saveError.value = errorText(error);
    }
  }

  async function flush() {
    if (timer != null) {
      window.clearTimeout(timer);
      timer = null;
    }
    if (!dirty) return;
    flushing ??= persist().finally(() => {
      flushing = null;
    });
    await flushing;
    if (dirty) await persist();
  }

  async function updateMeta(id: string, patch: NoteMetaPatch) {
    if (current.value?.id === id) {
      edit({
        pinned: patch.pinned ?? current.value.pinned,
        archived: patch.archived ?? current.value.archived,
        tags: patch.tags ?? current.value.tags,
        dueAt: patch.dueAt === undefined ? current.value.dueAt : patch.dueAt,
      });
      return;
    }
    const index = await setNoteMeta(id, patch);
    replaceIndex(index);
  }

  async function remove(id: string) {
    if (current.value?.id === id) {
      dirty = false;
      current.value = null;
      activeId.value = null;
    }
    await deleteNote(id);
    notes.value = notes.value.filter((item) => item.id !== id);
  }

  function closeEditor() {
    void flush();
    activeId.value = null;
  }

  return {
    notes,
    attachmentsDir,
    activeId,
    current,
    loading,
    loadError,
    saveState,
    saveError,
    savedAt,
    reload,
    open,
    create,
    edit,
    flush,
    updateMeta,
    remove,
    closeEditor,
  };
}

export function bindNoteLifecycle() {
  let unlisten: UnlistenFn | null = null;
  let unlistenClose: UnlistenFn | null = null;
  const { reload, flush } = useNotes();

  onMounted(async () => {
    if (listening) return;
    listening = true;
    try {
      unlisten = await listen("notes-changed", () => {
        void reload();
      });
      unlistenClose = await getCurrentWindow().onCloseRequested(async () => {
        await flush();
      });
    } catch {
      listening = false;
    }
  });

  onUnmounted(() => {
    unlisten?.();
    unlistenClose?.();
    listening = false;
    void flush();
  });
}

export type { NoteNode };
