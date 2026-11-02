import { invoke } from "@tauri-apps/api/core";
import type { Attachment, Note, NoteIndex, NoteMetaPatch, NoteSave, NotesIndexFile } from "../domain/notes";

export function loadNotesIndex() {
  return invoke<NotesIndexFile>("notes_index");
}

export function loadNote(id: string) {
  return invoke<Note>("note_load", { id });
}

export function createNote(seed?: string) {
  return invoke<Note>("note_create", { seed: seed ?? null });
}

export function saveNote(input: NoteSave) {
  return invoke<NoteIndex>("note_save", { input });
}

export function deleteNote(id: string) {
  return invoke<void>("note_delete", { id });
}

export function setNoteMeta(id: string, patch: NoteMetaPatch) {
  return invoke<NoteIndex>("note_set_meta", { id, patch });
}

export function prepareAttachment() {
  return invoke<string>("note_attachment_prepare");
}

export function commitAttachment(tmp: string, ext: string) {
  return invoke<Attachment>("note_attachment_commit", { tmp, ext });
}

export function importAttachment(path: string) {
  return invoke<Attachment>("note_attachment_import", { path });
}

export function beginAttachment() {
  return invoke<string>("note_attachment_begin");
}

export function pushAttachmentChunk(id: string, chunk: string) {
  return invoke<void>("note_attachment_chunk", { id, chunk });
}

export function finishAttachment(id: string, ext: string) {
  return invoke<Attachment>("note_attachment_finish", { id, ext });
}

export function exportNote(id: string, format: "md" | "html", path?: string) {
  return invoke<string>("note_export", { id, format, path: path ?? null });
}
