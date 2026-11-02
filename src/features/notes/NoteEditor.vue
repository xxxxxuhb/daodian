<script setup lang="ts">
import { NodeSelection } from "@tiptap/pm/state";
import type { Editor } from "@tiptap/core";
import { EditorContent, useEditor } from "@tiptap/vue-3";
import { computed, inject, nextTick, onMounted, ref, watch } from "vue";
import { useLibrary } from "../../data/useLibrary";
import { useNow } from "../../domain/clock";
import type { Note, NoteNode } from "../../domain/notes";
import { clockTime } from "../../domain/present";
import Icon from "../../ui/Icon.vue";
import TimeField from "../../ui/TimeField.vue";
import { tierKey } from "../shell/tier";
import { SLASH_ITEMS, noteExtensions } from "./editor/extensions";
import SlashMenu from "./editor/SlashMenu.vue";
import type { LinkedTarget } from "./open";
import { countImages, MAX_IMAGES, savedLabel } from "./logic";
import { pickImage, storeImage } from "./upload";

const props = defineProps<{
  note: Note;
  saveState: "saved" | "saving" | "dirty" | "error";
  saveError: string;
  savedAt: number | null;
  focusTitle: boolean;
  query: string;
}>();
const emit = defineEmits<{
  "update:title": [string];
  "update:doc": [NoteNode];
  "update:tags": [string[]];
  "update:due": [number | null];
  "toggle-pin": [];
  "toggle-archive": [];
  remove: [];
  export: ["md" | "html"];
  back: [];
  "focus-consumed": [];
  error: [string];
}>();

const now = useNow();
const tier = inject(tierKey, computed(() => "standard" as const));
const compact = computed(() => tier.value === "mini");
const moreOpen = ref(false);
const tagDraft = ref("");
const tagging = ref(false);
const uploading = ref(false);
const linkOpen = ref(false);
const linkValue = ref("");
const fileEl = ref<HTMLInputElement | null>(null);
const titleEl = ref<HTMLInputElement | null>(null);
const rev = ref(0);
const slash = ref<{ query: string; from: number } | null>(null);
const slashIndex = ref(0);
const mention = ref<{ query: string; from: number } | null>(null);
const mentionIndex = ref(0);
const menuPos = ref({ top: 0, left: 0 });
const bubble = ref<{ top: number; left: number } | null>(null);
const tableBar = ref<{ top: number; left: number } | null>(null);
const { todos, plans } = useLibrary();
let applying = false;

const editor = useEditor({
  content: props.note.doc,
  extensions: noteExtensions(),
  editorProps: {
    attributes: { class: "note-prose" },
    handlePaste: (_view, event) => takeFiles(event.clipboardData?.files),
    handleDrop: (_view, event) => takeFiles(event.dataTransfer?.files),
    handleKeyDown: (_view, event) => onEditorKey(event),
  },
  onUpdate: ({ editor: instance }) => {
    if (applying) return;
    emit("update:doc", instance.getJSON() as NoteNode);
    syncMenus(instance);
  },
  onSelectionUpdate: ({ editor: instance }) => {
    rev.value += 1;
    syncMenus(instance);
  },
});

const active = computed(() => {
  rev.value;
  const instance = editor.value;
  return {
    h1: !!instance?.isActive("heading", { level: 1 }),
    h2: !!instance?.isActive("heading", { level: 2 }),
    bold: !!instance?.isActive("bold"),
    italic: !!instance?.isActive("italic"),
    underline: !!instance?.isActive("underline"),
    highlight: !!instance?.isActive("highlight"),
    bullet: !!instance?.isActive("bulletList"),
    quote: !!instance?.isActive("blockquote"),
    task: !!instance?.isActive("taskList"),
    table: !!instance?.isActive("table"),
    link: !!instance?.isActive("link"),
  };
});

const filteredSlash = computed(() => {
  const query = slash.value?.query.trim().toLowerCase() ?? "";
  return SLASH_ITEMS.filter((item) => !query || item.label.includes(query) || item.id.includes(query));
});

const mentionItems = computed(() => {
  const query = mention.value?.query.trim().toLowerCase() ?? "";
  const rows: LinkedTarget[] = [];
  for (const todo of todos.value) {
    if (!query || todo.title.toLowerCase().includes(query)) {
      rows.push({ kind: "todo", id: todo.id, title: todo.title });
    }
  }
  for (const plan of plans.value) {
    if (!query || plan.title.toLowerCase().includes(query)) {
      rows.push({ kind: "plan", id: plan.id, title: plan.title });
    }
    for (const stage of plan.stages) {
      const title = `${plan.title} · ${stage.title}`;
      if (!query || title.toLowerCase().includes(query)) {
        rows.push({ kind: "stage", id: stage.id, planId: plan.id, title });
      }
    }
  }
  return rows.slice(0, 8);
});

const statusText = computed(() => {
  if (uploading.value) return "正在插入图片";
  if (props.saveState === "saving" || props.saveState === "dirty") return "保存中";
  if (props.saveState === "error") return props.saveError || "没保存上";
  if (props.savedAt) return `已保存 ${savedLabel(props.savedAt)}`;
  return "已保存";
});

watch(
  () => [props.note.id, props.note.updatedAt] as const,
  async (next, prev) => {
    const instance = editor.value;
    if (!instance) return;
    const sameDoc = JSON.stringify(instance.getJSON()) === JSON.stringify(props.note.doc);
    if (prev && next[0] === prev[0] && sameDoc) return;
    applying = true;
    instance.commands.setContent(props.note.doc, false);
    applying = false;
    highlight(instance, props.query);
    slash.value = null;
    mention.value = null;
    bubble.value = null;
    tableBar.value = null;
  },
);

watch(
  () => props.focusTitle,
  async (focus) => {
    if (!focus) return;
    await nextTick();
    titleEl.value?.focus();
    titleEl.value?.select();
    emit("focus-consumed");
  },
  { immediate: true },
);

onMounted(() => {
  nextTick(() => {
    const instance = editor.value;
    if (instance) highlight(instance, props.query);
  });
});

watch(
  () => props.query,
  () => {
    const instance = editor.value;
    if (instance) highlight(instance, props.query);
  },
);

function chain() {
  return editor.value?.chain().focus();
}

function mark(name: "bold" | "italic" | "underline" | "highlight") {
  const run = chain();
  if (!run) return;
  if (name === "bold") run.toggleBold().run();
  if (name === "italic") run.toggleItalic().run();
  if (name === "underline") run.toggleUnderline().run();
  if (name === "highlight") run.toggleHighlight().run();
}

function block(kind: string) {
  const run = chain();
  if (!run) return;
  if (kind === "h1") run.toggleHeading({ level: 1 }).run();
  if (kind === "h2") run.toggleHeading({ level: 2 }).run();
  if (kind === "bullet") run.toggleBulletList().run();
  if (kind === "quote") run.toggleBlockquote().run();
  if (kind === "task") run.toggleTaskList().run();
  if (kind === "table") run.insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run();
}

function isImage(file: File) {
  return file.type.startsWith("image/") || /\.(png|jpe?g|gif|webp|bmp)$/i.test(file.name);
}

function takeFiles(list: FileList | undefined | null) {
  const files = [...(list ?? [])].filter(isImage);
  if (!files.length) return false;
  void addFiles(files);
  return true;
}

async function addFiles(files: File[]) {
  const instance = editor.value;
  if (!instance) return;
  for (const file of files) {
    if (countImages(instance.getJSON() as NoteNode) >= MAX_IMAGES) {
      emit("error", "一篇笔记最多 200 张图");
      return;
    }
    uploading.value = true;
    try {
      const stored = await storeImage(file);
      instance
        .chain()
        .focus()
        .insertContent({ type: "image", attrs: { hash: stored.hash, ext: stored.ext, alt: "" } })
        .run();
    } catch (error) {
      emit("error", error instanceof Error ? error.message : String(error));
    } finally {
      uploading.value = false;
    }
  }
}

async function chooseImage() {
  try {
    const stored = await pickImage();
    if (!stored || !editor.value) return;
    if (countImages(editor.value.getJSON() as NoteNode) >= MAX_IMAGES) {
      emit("error", "一篇笔记最多 200 张图");
      return;
    }
    uploading.value = true;
    editor.value.chain().focus().insertContent({ type: "image", attrs: { hash: stored.hash, ext: stored.ext, alt: "" } }).run();
  } catch (error) {
    emit("error", error instanceof Error ? error.message : String(error));
  } finally {
    uploading.value = false;
  }
}

function onFile(event: Event) {
  const input = event.target as HTMLInputElement;
  void addFiles([...(input.files ?? [])]);
  input.value = "";
}

function askLink() {
  const previous = editor.value?.getAttributes("link").href;
  linkValue.value = typeof previous === "string" ? previous : "";
  linkOpen.value = true;
}

function applyLink() {
  const instance = editor.value;
  if (!instance) return;
  const href = linkValue.value.trim();
  if (!href) instance.chain().focus().unsetLink().run();
  else instance.chain().focus().extendMarkRange("link").setLink({ href }).run();
  linkOpen.value = false;
}

function syncMenus(instance: Editor) {
  slash.value = readToken(instance, "/");
  mention.value = slash.value ? null : readToken(instance, "@");
  if (slash.value) slashIndex.value = Math.min(slashIndex.value, Math.max(filteredSlash.value.length - 1, 0));
  if (mention.value) mentionIndex.value = Math.min(mentionIndex.value, Math.max(mentionItems.value.length - 1, 0));
  const token = slash.value ?? mention.value;
  if (token) {
    const coords = instance.view.coordsAtPos(token.from);
    menuPos.value = { top: coords.bottom + 6, left: coords.left };
  }
  tableBar.value = tableAnchor(instance);
  const { from, empty } = instance.state.selection;
  if (empty || instance.state.selection instanceof NodeSelection) {
    bubble.value = null;
    return;
  }
  const coords = instance.view.coordsAtPos(from);
  bubble.value = { top: Math.max(8, coords.top - 36), left: coords.left };
}

function tableAnchor(instance: Editor) {
  const { $from } = instance.state.selection;
  for (let depth = $from.depth; depth > 0; depth -= 1) {
    if ($from.node(depth).type.name !== "table") continue;
    const coords = instance.view.coordsAtPos($from.before(depth) + 1);
    return { top: Math.max(8, coords.top - 40), left: Math.max(8, coords.left) };
  }
  return null;
}

function resizeTable(mode: "fill" | "even") {
  const instance = editor.value;
  if (!instance) return;
  const { $from } = instance.state.selection;
  let tablePos = -1;
  let table = $from.node(0);
  for (let depth = $from.depth; depth > 0; depth -= 1) {
    if ($from.node(depth).type.name !== "table") continue;
    tablePos = $from.before(depth);
    table = $from.node(depth);
    break;
  }
  const first = tablePos >= 0 ? table.firstChild : null;
  const host = tablePos >= 0 ? instance.view.nodeDOM(tablePos) : null;
  const target = host instanceof HTMLElement ? host.clientWidth : 0;
  if (!first || target <= 0) return;
  const current: number[] = [];
  first.forEach((cell) => {
    const span = Number(cell.attrs.colspan) || 1;
    const widths = cell.attrs.colwidth as number[] | null;
    for (let index = 0; index < span; index += 1) current.push(Number(widths?.[index]) || 0);
  });
  const count = current.length;
  if (!count) return;
  const next =
    mode === "even"
      ? evenWidths(target, count)
      : scaledWidths(
          current.some((width) => width > 0) ? current : current.map(() => 1),
          target,
        );
  const tr = instance.state.tr;
  let rowPos = tablePos + 1;
  table.forEach((row) => {
    let column = 0;
    let cellPos = rowPos + 1;
    row.forEach((cell) => {
      const span = Number(cell.attrs.colspan) || 1;
      const colwidth = next.slice(column, column + span);
      const previous = (cell.attrs.colwidth as number[] | null) ?? [];
      const same = previous.length === colwidth.length && previous.every((width, index) => width === colwidth[index]);
      if (!same) tr.setNodeMarkup(cellPos, undefined, { ...cell.attrs, colwidth });
      column += span;
      cellPos += cell.nodeSize;
    });
    rowPos += row.nodeSize;
  });
  if (tr.docChanged) instance.view.dispatch(tr);
}

function evenWidths(target: number, count: number) {
  const each = Math.floor(target / count);
  const widths = Array.from({ length: count }, () => each);
  widths[count - 1] = target - each * (count - 1);
  return widths;
}

function scaledWidths(current: number[], target: number) {
  const known = current.filter((width) => width > 0);
  const fallback = known.length ? known.reduce((total, width) => total + width, 0) / known.length : 1;
  const base = current.map((width) => width || fallback);
  const sum = base.reduce((total, width) => total + width, 0) || 1;
  const widths = base.map((width) => Math.max(1, Math.round((width / sum) * target)));
  widths[widths.length - 1] += target - widths.reduce((total, width) => total + width, 0);
  return widths;
}

function readToken(instance: Editor, mark: "/" | "@") {
  const { $from } = instance.state.selection;
  if (!$from.parent.isTextblock) return null;
  const text = $from.parent.textBetween(0, $from.parentOffset, "\n", "\0");
  const matcher = mark === "/" ? /(?:^|\s)\/([^\s/]*)$/ : /(?:^|\s)@([^\s@]*)$/;
  const found = matcher.exec(text);
  if (!found) return null;
  const query = found[1] ?? "";
  return { query, from: $from.pos - query.length - 1 };
}

function onEditorKey(event: KeyboardEvent) {
  if (slash.value && filteredSlash.value.length) {
    if (event.key === "ArrowDown") {
      slashIndex.value = (slashIndex.value + 1) % filteredSlash.value.length;
      return true;
    }
    if (event.key === "ArrowUp") {
      slashIndex.value = (slashIndex.value + filteredSlash.value.length - 1) % filteredSlash.value.length;
      return true;
    }
    if (event.key === "Enter") {
      const item = filteredSlash.value[slashIndex.value];
      if (item) runSlash(item.id);
      return true;
    }
    if (event.key === "Escape") {
      slash.value = null;
      return true;
    }
  }
  if (mention.value && mentionItems.value.length) {
    if (event.key === "ArrowDown") {
      mentionIndex.value = (mentionIndex.value + 1) % mentionItems.value.length;
      return true;
    }
    if (event.key === "ArrowUp") {
      mentionIndex.value = (mentionIndex.value + mentionItems.value.length - 1) % mentionItems.value.length;
      return true;
    }
    if (event.key === "Enter") {
      const item = mentionItems.value[mentionIndex.value];
      if (item) insertRef(item);
      return true;
    }
    if (event.key === "Escape") {
      mention.value = null;
      return true;
    }
  }
  return false;
}

function runSlash(id: string) {
  const state = slash.value;
  const instance = editor.value;
  if (!state || !instance) return;
  const run = instance.chain().focus().deleteRange({ from: state.from, to: instance.state.selection.from });
  slash.value = null;
  if (id === "h1") run.toggleHeading({ level: 1 }).run();
  else if (id === "h2") run.toggleHeading({ level: 2 }).run();
  else if (id === "bullet") run.toggleBulletList().run();
  else if (id === "ordered") run.toggleOrderedList().run();
  else if (id === "task") run.toggleTaskList().run();
  else if (id === "quote") run.toggleBlockquote().run();
  else if (id === "code") run.toggleCodeBlock().run();
  else if (id === "rule") run.setHorizontalRule().run();
  else if (id === "table") run.insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run();
  else if (id === "image") {
    run.run();
    void chooseImage();
  }
}

function insertRef(target: LinkedTarget) {
  const state = mention.value;
  const instance = editor.value;
  if (!state || !instance) return;
  instance
    .chain()
    .focus()
    .deleteRange({ from: state.from, to: instance.state.selection.from })
    .insertContent({
      type: "noteRef",
      attrs: { kind: target.kind, id: target.id, planId: target.planId ?? "", title: target.title },
    })
    .run();
  mention.value = null;
}

function highlight(instance: Editor, query: string) {
  const keyword = query.trim().toLowerCase();
  if (keyword.length < 1) return;
  let found: { from: number; to: number } | null = null;
  instance.state.doc.descendants((node, pos) => {
    if (found || !node.isText) return !found;
    const text = node.text ?? "";
    const index = text.toLowerCase().indexOf(keyword);
    if (index >= 0) {
      found = { from: pos + index, to: pos + index + keyword.length };
      return false;
    }
    return true;
  });
  if (!found) return;
  const range = found;
  instance.commands.setTextSelection(range);
  instance.commands.scrollIntoView();
}

function commitTag() {
  const tag = tagDraft.value.trim().replace(/^#/, "");
  tagDraft.value = "";
  tagging.value = false;
  if (!tag || props.note.tags.includes(tag)) return;
  emit("update:tags", [...props.note.tags, tag].slice(0, 12));
}

function dropTag(tag: string) {
  emit(
    "update:tags",
    props.note.tags.filter((item) => item !== tag),
  );
}

function onDrag(event: DragEvent) {
  if ([...(event.dataTransfer?.files ?? [])].some(isImage)) event.preventDefault();
}

function reposition() {
  const instance = editor.value;
  if (instance) syncMenus(instance);
}
</script>

<template>
  <section class="note-editor" @dragover="onDrag" @drop.prevent="takeFiles($event.dataTransfer?.files)">
    <div class="etools">
      <button v-if="compact" type="button" class="tool" @click="emit('back')">返回</button>
      <button type="button" class="tool" :disabled="!editor?.can().undo()" @click="chain()?.undo().run()">撤销</button>
      <button type="button" class="tool" :disabled="!editor?.can().redo()" @click="chain()?.redo().run()">重做</button>
      <i class="sep"></i>
      <button type="button" class="tool" :class="{ on: active.h1 }" @click="block('h1')">H1</button>
      <button type="button" class="tool" :class="{ on: active.h2 }" @click="block('h2')">H2</button>
      <i class="sep"></i>
      <button type="button" class="tool" :class="{ on: active.bold }" @click="mark('bold')"><b>B</b></button>
      <button type="button" class="tool" :class="{ on: active.italic }" @click="mark('italic')"><i>I</i></button>
      <button type="button" class="tool" :class="{ on: active.underline }" @click="mark('underline')"><u>U</u></button>
      <template v-if="!compact || moreOpen">
        <i class="sep"></i>
        <button type="button" class="tool icon" :class="{ on: active.bullet }" title="列表" aria-label="列表" @click="block('bullet')"><Icon name="list" /></button>
        <button type="button" class="tool icon" :class="{ on: active.quote }" title="引用" aria-label="引用" @click="block('quote')"><Icon name="quote" /></button>
        <button type="button" class="tool icon" :class="{ on: active.task }" title="清单" aria-label="清单" @click="block('task')"><Icon name="task" /></button>
        <button type="button" class="tool icon" :class="{ on: active.table }" title="表格" aria-label="表格" @click="block('table')"><Icon name="table" /></button>
        <button type="button" class="tool icon" title="图片" aria-label="图片" @click="fileEl?.click()"><Icon name="image" /></button>
        <button type="button" class="tool icon" :class="{ on: active.link }" title="链接" aria-label="链接" @click="askLink"><Icon name="link" /></button>
        <button type="button" class="tool icon" :class="{ on: active.highlight }" title="高亮" aria-label="高亮" @click="mark('highlight')"><Icon name="mark" /></button>
      </template>
      <button v-if="compact" type="button" class="tool" @click="moreOpen = !moreOpen">⋯</button>
      <span class="right">{{ statusText }}</span>
      <div class="ops">
        <button type="button" class="tool">操作</button>
        <div class="note-menu" @mousedown.stop>
          <button type="button" @click="emit('toggle-pin')">{{ note.pinned ? "取消置顶" : "置顶" }}</button>
          <button type="button" @click="emit('toggle-archive')">{{ note.archived ? "取消归档" : "归档" }}</button>
          <button type="button" @click="emit('export', 'md')">导出 Markdown</button>
          <button type="button" @click="emit('export', 'html')">导出 HTML</button>
          <button type="button" class="danger" @click="emit('remove')">删除</button>
        </div>
      </div>
    </div>
    <div class="doc" @scroll="reposition">
      <div class="inner">
        <input
          ref="titleEl"
          class="note-title"
          :value="note.title"
          placeholder="未命名"
          @input="emit('update:title', ($event.target as HTMLInputElement).value)"
        />
        <div class="sub">
          <button v-for="tag in note.tags" :key="tag" type="button" class="tag" @click="dropTag(tag)">#{{ tag }}</button>
          <input
            v-if="tagging"
            v-model="tagDraft"
            class="tag-input"
            placeholder="标签"
            @keydown.enter.prevent="commitTag"
            @keydown.esc="tagging = false"
            @blur="commitTag"
          />
          <button v-else type="button" class="tag add" @click="tagging = true">+ 标签</button>
          <span>编辑 {{ clockTime(note.updatedAt) }}</span>
          <span>·</span>
          <TimeField :at="note.dueAt ?? null" :now="now" @update:at="emit('update:due', $event)" />
          <button v-if="note.dueAt" type="button" class="tag add" @click="emit('update:due', null)">清除到点</button>
        </div>
        <p v-if="saveState === 'error'" class="banner">{{ saveError }}</p>
        <editor-content :editor="editor" />
      </div>
    </div>
    <input ref="fileEl" class="file" type="file" accept="image/png,image/jpeg,image/gif,image/webp,image/bmp" multiple @change="onFile" />
    <SlashMenu
      v-if="slash && filteredSlash.length"
      class="pop"
      :style="{ top: menuPos.top + 'px', left: menuPos.left + 'px' }"
      :items="filteredSlash"
      :active="slashIndex"
      @pick="runSlash"
    />
    <div v-if="mention && mentionItems.length" class="slash pop" :style="{ top: menuPos.top + 'px', left: menuPos.left + 'px' }">
      <button
        v-for="(item, index) in mentionItems"
        :key="item.kind + item.id"
        type="button"
        :class="{ on: index === mentionIndex }"
        @mousedown.prevent="insertRef(item)"
      >
        {{ item.kind === "todo" ? "待办" : item.kind === "plan" ? "计划" : "阶段" }} · {{ item.title }}
      </button>
    </div>
    <div v-if="tableBar" class="bubble" :style="{ top: tableBar.top + 'px', left: tableBar.left + 'px' }">
      <button type="button" @mousedown.prevent="chain()?.addRowAfter().run()">加行</button>
      <button type="button" @mousedown.prevent="chain()?.addColumnAfter().run()">加列</button>
      <button type="button" @mousedown.prevent="chain()?.deleteRow().run()">删行</button>
      <button type="button" @mousedown.prevent="chain()?.deleteColumn().run()">删列</button>
      <button type="button" @mousedown.prevent="chain()?.mergeCells().run()">合并</button>
      <button type="button" @mousedown.prevent="chain()?.splitCell().run()">拆分</button>
      <button type="button" @mousedown.prevent="chain()?.toggleHeaderRow().run()">表头</button>
      <button type="button" @mousedown.prevent="resizeTable('fill')">占满</button>
      <button type="button" @mousedown.prevent="resizeTable('even')">平分</button>
    </div>
    <div v-if="bubble" class="bubble" :style="{ top: bubble.top + 'px', left: bubble.left + 'px' }">
      <button type="button" :class="{ on: active.bold }" @mousedown.prevent="mark('bold')">B</button>
      <button type="button" :class="{ on: active.italic }" @mousedown.prevent="mark('italic')">I</button>
      <button type="button" :class="{ on: active.highlight }" @mousedown.prevent="mark('highlight')">高亮</button>
      <button type="button" :class="{ on: active.link }" @mousedown.prevent="askLink">链接</button>
    </div>
    <form v-if="linkOpen" class="link-pop" @submit.prevent="applyLink">
      <input v-model="linkValue" type="url" placeholder="https://" />
      <button type="submit">确定</button>
      <button type="button" @click="linkOpen = false">取消</button>
    </form>
  </section>
</template>
