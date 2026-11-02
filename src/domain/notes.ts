export interface NoteMark {
  type: string;
  attrs?: Record<string, unknown>;
}

export interface NoteNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: NoteNode[];
  text?: string;
  marks?: NoteMark[];
}

export interface NoteIndex {
  id: string;
  title: string;
  summary: string;
  text: string;
  tags: string[];
  dueAt?: number | null;
  pinned: boolean;
  archived: boolean;
  reminded: boolean;
  thumb?: string | null;
  attachments: string[];
  createdAt: number;
  updatedAt: number;
}

export interface Note extends NoteIndex {
  doc: NoteNode;
  docVersion: number;
}

export interface NotesIndexFile {
  version: number;
  attachmentsDir: string;
  notes: NoteIndex[];
}

export interface NoteSave {
  id: string;
  title: string;
  doc: NoteNode;
  docVersion: number;
  tags: string[];
  dueAt: number | null;
  pinned: boolean;
  archived: boolean;
  thumb: string | null;
  attachments: string[];
  text: string;
  summary: string;
  baseUpdatedAt: number;
}

export interface NoteMetaPatch {
  pinned?: boolean;
  archived?: boolean;
  tags?: string[];
  dueAt?: number | null;
}

export interface Attachment {
  hash: string;
  ext: string;
  thumb?: string | null;
}

export function emptyDoc(): NoteNode {
  return { type: "doc", content: [{ type: "paragraph" }] };
}
