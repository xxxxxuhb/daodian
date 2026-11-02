import { mergeAttributes, Node } from "@tiptap/core";
import Highlight from "@tiptap/extension-highlight";
import Link from "@tiptap/extension-link";
import Placeholder from "@tiptap/extension-placeholder";
import Table from "@tiptap/extension-table";
import TableCell from "@tiptap/extension-table-cell";
import TableHeader from "@tiptap/extension-table-header";
import TableRow from "@tiptap/extension-table-row";
import TaskItem from "@tiptap/extension-task-item";
import TaskList from "@tiptap/extension-task-list";
import TextAlign from "@tiptap/extension-text-align";
import Underline from "@tiptap/extension-underline";
import StarterKit from "@tiptap/starter-kit";
import { VueNodeViewRenderer } from "@tiptap/vue-3";
import ImageView from "./ImageView.vue";
import NoteRefView from "./NoteRefView.vue";

export const NoteImage = Node.create({
  name: "image",
  group: "block",
  atom: true,
  draggable: true,
  addAttributes() {
    return {
      hash: { default: "" },
      ext: { default: "png" },
      alt: { default: "" },
    };
  },
  parseHTML() {
    return [{ tag: "img[data-hash]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["img", mergeAttributes(HTMLAttributes, { "data-hash": HTMLAttributes.hash })];
  },
  addNodeView() {
    return VueNodeViewRenderer(ImageView);
  },
});

export const NoteRef = Node.create({
  name: "noteRef",
  group: "block",
  atom: true,
  selectable: true,
  addAttributes() {
    return {
      kind: { default: "todo" },
      id: { default: "" },
      planId: { default: "" },
      title: { default: "" },
    };
  },
  parseHTML() {
    return [{ tag: "div[data-note-ref]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["div", mergeAttributes({ "data-note-ref": HTMLAttributes.kind }, HTMLAttributes)];
  },
  addNodeView() {
    return VueNodeViewRenderer(NoteRefView);
  },
});

export function noteExtensions() {
  return [
    StarterKit.configure({ heading: { levels: [1, 2, 3] } }),
    Underline,
    Highlight,
    Link.configure({ openOnClick: false, autolink: true, linkOnPaste: true }),
    Placeholder.configure({ placeholder: "写下来，或输入 / 插入表格、图片、清单" }),
    TextAlign.configure({ types: ["heading", "paragraph"] }),
    Table.configure({ resizable: true }),
    TableRow,
    TableHeader,
    TableCell,
    TaskList,
    TaskItem.configure({ nested: true }),
    NoteImage,
    NoteRef,
  ];
}

export const SLASH_ITEMS = [
  { id: "h1", label: "一级标题" },
  { id: "h2", label: "二级标题" },
  { id: "bullet", label: "无序列表" },
  { id: "ordered", label: "有序列表" },
  { id: "task", label: "待办清单" },
  { id: "quote", label: "引用" },
  { id: "code", label: "代码块" },
  { id: "rule", label: "分割线" },
  { id: "table", label: "表格" },
  { id: "image", label: "图片" },
];
