<script setup lang="ts">
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { downloadDir, join } from "@tauri-apps/api/path";
import { computed, ref, watch } from "vue";
import { utils as xlsxUtils, write as writeWorkbook } from "xlsx";
import { testDeepSeek } from "../../api/ai";
import { loadLibrary } from "../../api/library";
import { testWebhook } from "../../api/settings";
import { useCalendar } from "../../data/useCalendar";
import { useSettings } from "../../data/useSettings";
import type { Settings } from "../../domain/settings";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();
const { settings, saving, error, update } = useSettings();
const { refresh: refreshCalendar, remove: removeCalendar, loading: calendarLoading } = useCalendar();
const draft = ref<Settings>(clone(settings.value));
const apiKey = ref("");
const clearApiKey = ref(false);
const saved = ref(false);
const newHoliday = ref("");
const newWorkday = ref("");
const testingWebhook = ref<"feishu" | "wecom" | "">("");
const testingDeepSeek = ref(false);
const exportingRecords = ref(false);
const exportStatus = ref("");
const externalEnabled = computed(
  () =>
    draft.value.calendar.enabled ||
    draft.value.feishu.enabled ||
    draft.value.wecom.enabled ||
    draft.value.quickCapture.enabled ||
    draft.value.deepSeek.enabled,
);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    draft.value = clone(settings.value);
    apiKey.value = "";
    clearApiKey.value = false;
    newHoliday.value = "";
    newWorkday.value = "";
    saved.value = false;
  },
);

watch(
  () => [settings.value.feishu.lastStatus, settings.value.wecom.lastStatus],
  ([feishuStatus, wecomStatus]) => {
    if (!props.open) return;
    draft.value.feishu.lastStatus = feishuStatus;
    draft.value.wecom.lastStatus = wecomStatus;
  },
);

function clone(value: Settings) {
  return JSON.parse(JSON.stringify(value)) as Settings;
}

function addDate(kind: "holidays" | "workdays") {
  const source = kind === "holidays" ? newHoliday : newWorkday;
  const value = source.value;
  if (!value) return;
  draft.value.repeat[kind] = [...new Set([...draft.value.repeat[kind], value])].sort();
  source.value = "";
}

function removeDate(kind: "holidays" | "workdays", value: string) {
  draft.value.repeat[kind] = draft.value.repeat[kind].filter((item) => item !== value);
}

async function chooseCalendar() {
  const selected = await openDialog({
    multiple: false,
    directory: false,
    filters: [{ name: "iCalendar", extensions: ["ics"] }],
  });
  if (typeof selected !== "string") return;
  draft.value.calendar.path = selected;
  draft.value.calendar.enabled = true;
  await refreshCalendarFile();
}

async function refreshCalendarFile() {
  try {
    const result = await refreshCalendar(draft.value.calendar.path);
    draft.value.calendar.lastStatus = result.errors.length
      ? `${result.status}：${result.errors.join("；")}`
      : result.status;
  } catch (cause) {
    draft.value.calendar.lastStatus = cause instanceof Error ? cause.message : String(cause);
  }
}

function clearCalendar() {
  draft.value.calendar = { enabled: false, path: "", lastStatus: "" };
  removeCalendar();
}

async function save() {
  saved.value = false;
  await update(
    (next) => Object.assign(next, clone(draft.value)),
    { apiKey: apiKey.value || undefined, clearApiKey: clearApiKey.value },
  ).catch(() => undefined);
  if (!error.value) {
    draft.value.quickCapture = clone(settings.value).quickCapture;
    saved.value = true;
    apiKey.value = "";
    clearApiKey.value = false;
    emit("close");
  }
}

async function testRobot(provider: "feishu" | "wecom") {
  testingWebhook.value = provider;
  try {
    draft.value[provider].lastStatus = await testWebhook(
      provider,
      draft.value[provider].webhook,
    );
  } catch (cause) {
    draft.value[provider].lastStatus =
      cause instanceof Error ? cause.message : String(cause);
  } finally {
    testingWebhook.value = "";
  }
}

async function testAi() {
  testingDeepSeek.value = true;
  await save();
  if (error.value) {
    testingDeepSeek.value = false;
    return;
  }
  try {
    draft.value.deepSeek.lastStatus = await testDeepSeek();
  } catch (cause) {
    draft.value.deepSeek.lastStatus =
      cause instanceof Error ? cause.message : String(cause);
  } finally {
    const status = draft.value.deepSeek.lastStatus;
    await update((next) => {
      next.deepSeek.lastStatus = status;
    }).catch(() => undefined);
    testingDeepSeek.value = false;
  }
}

async function exportAllRecords() {
  exportingRecords.value = true;
  exportStatus.value = "";
  try {
    const date = new Date().toISOString().slice(0, 10);
    const defaultPath = await join(
      await downloadDir(),
      `到点全部记录-${date}.xlsx`,
    );
    const path = await saveDialog({
      defaultPath,
      filters: [{ name: "Excel 工作簿", extensions: ["xlsx"] }],
    });
    if (!path) return;
    const library = await loadLibrary();
    const todoHeaders = [
      "ID", "标题", "备注", "到点时间", "优先级", "状态", "重复规则",
      "提前提醒（分钟）", "创建时间", "更新时间",
    ];
    const todoRows = library.todos.map((todo) => ({
      ID: todo.id,
      标题: todo.title,
      备注: todo.note,
      到点时间: displayTime(todo.dueAt),
      优先级: priorityLabel(todo.priority),
      状态: todo.done ? "已完成" : todo.skipped ? "已跳过" : "未完成",
      重复规则: todo.repeat ? JSON.stringify(todo.repeat) : "",
      "提前提醒（分钟）": Math.round(todo.leadMs / 60_000),
      创建时间: displayTime(todo.createdAt),
      更新时间: displayTime(todo.updatedAt),
    }));
    const planHeaders = [
      "计划ID", "计划标题", "计划备注", "优先级", "计划状态", "模板ID",
      "阶段序号", "阶段ID", "阶段标题", "阶段时间", "阶段状态",
      "依赖阶段ID", "依赖延迟（小时）", "创建时间", "更新时间",
    ];
    const planRows = library.plans.flatMap((plan) => {
      const stages = plan.stages.length ? plan.stages : [null];
      return stages.map((stage, index) => ({
        计划ID: plan.id,
        计划标题: plan.title,
        计划备注: plan.note,
        优先级: priorityLabel(plan.priority),
        计划状态: plan.done ? "已完成" : "进行中",
        模板ID: plan.templateId ?? "",
        阶段序号: stage ? index + 1 : "",
        阶段ID: stage?.id ?? "",
        阶段标题: stage?.title ?? "",
        阶段时间: displayTime(stage?.at ?? null),
        阶段状态: stage ? (stage.done ? "已完成" : "未完成") : "",
        依赖阶段ID: stage?.dependsOn?.stageId ?? "",
        "依赖延迟（小时）": stage?.dependsOn ? stage.dependsOn.delayMs / 3_600_000 : "",
        创建时间: displayTime(plan.createdAt),
        更新时间: displayTime(plan.updatedAt),
      }));
    });
    const workbook = xlsxUtils.book_new();
    xlsxUtils.book_append_sheet(
      workbook,
      sheetWithWidths(todoRows, todoHeaders),
      "待办",
    );
    xlsxUtils.book_append_sheet(
      workbook,
      sheetWithWidths(planRows, planHeaders),
      "计划",
    );
    const output = writeWorkbook(workbook, {
      type: "array",
      bookType: "xlsx",
      compression: true,
    });
    await writeFile(path, new Uint8Array(output));
    exportStatus.value = `已导出 ${library.todos.length} 条待办、${library.plans.length} 份计划`;
  } catch (cause) {
    exportStatus.value = `导出失败：${cause instanceof Error ? cause.message : String(cause)}`;
  } finally {
    exportingRecords.value = false;
  }
}

function displayTime(value: number | null) {
  return value == null ? "" : new Date(value).toLocaleString("zh-CN", { hour12: false });
}

function priorityLabel(value: "low" | "normal" | "high") {
  return value === "high" ? "重要" : value === "low" ? "可缓" : "普通";
}

function sheetWithWidths(rows: Record<string, unknown>[], headers: string[]) {
  const sheet = xlsxUtils.json_to_sheet(rows, { header: headers });
  sheet["!cols"] = headers.map((header) => ({
    wch: Math.min(
      40,
      Math.max(
        header.length + 2,
        ...rows.map((row) => String(row[header] ?? "").length + 2),
      ),
    ),
  }));
  return sheet;
}
</script>

<template>
  <div v-if="props.open" class="settings-scrim" @mousedown.self="emit('close')">
    <aside class="settings-drawer" aria-label="设置">
      <header class="settings-head">
        <div>
          <p class="eyebrow">SETTINGS</p>
          <h2>设置</h2>
        </div>
        <button type="button" class="settings-close" aria-label="关闭设置" @click="emit('close')">×</button>
      </header>

      <div class="settings-body">
        <section class="settings-section">
          <h3>外观与窗口</h3>
          <label class="settings-field">
            <span>主题</span>
            <select v-model="draft.theme">
              <option value="celadon">青瓷</option>
              <option value="night">夜航</option>
            </select>
          </label>
          <p class="settings-note">
            窗口会按迷你、标准、宽屏三档分别记住大小与位置；上次档位：{{ draft.window.lastTier || "尚未记录" }}。
          </p>
        </section>

        <section class="settings-section">
          <h3>重复与工作日</h3>
          <label class="settings-check">
            <input v-model="draft.repeat.defaultSkipWeekends" type="checkbox" />
            <span>新建重复待办默认跳过周末</span>
          </label>
          <div class="settings-date-group">
            <span>节假日</span>
            <div class="settings-date-add">
              <input v-model="newHoliday" type="date" />
              <button type="button" class="btn btn--sm" :disabled="!newHoliday" @click="addDate('holidays')">添加</button>
            </div>
            <div v-if="draft.repeat.holidays.length" class="settings-date-list">
              <span v-for="date in draft.repeat.holidays" :key="date">
                {{ date }}
                <button type="button" :aria-label="`删除节假日 ${date}`" @click="removeDate('holidays', date)">×</button>
              </span>
            </div>
            <small v-else>尚未添加节假日</small>
          </div>
          <div class="settings-date-group">
            <span>调休工作日</span>
            <div class="settings-date-add">
              <input v-model="newWorkday" type="date" />
              <button type="button" class="btn btn--sm" :disabled="!newWorkday" @click="addDate('workdays')">添加</button>
            </div>
            <div v-if="draft.repeat.workdays.length" class="settings-date-list">
              <span v-for="date in draft.repeat.workdays" :key="date">
                {{ date }}
                <button type="button" :aria-label="`删除调休工作日 ${date}`" @click="removeDate('workdays', date)">×</button>
              </span>
            </div>
            <small v-else>尚未添加调休工作日；同一天也在节假日时，以工作日为准</small>
          </div>
        </section>

        <section class="settings-section">
          <h3>工作时间</h3>
          <div class="settings-work-hours">
            <span>上午</span>
            <input v-model="draft.workHours.morningStart" type="time" />
            <i>至</i>
            <input v-model="draft.workHours.morningEnd" type="time" />
            <span>下午</span>
            <input v-model="draft.workHours.afternoonStart" type="time" />
            <i>至</i>
            <input v-model="draft.workHours.afternoonEnd" type="time" />
          </div>
          <p class="settings-note">AI 解析待办和生成计划时，会优先把时间安排在以上两个工作时段内。</p>
        </section>

        <section class="settings-section">
          <h3>周报</h3>
          <label class="settings-field">
            <span>统计周偏移</span>
            <input v-model.number="draft.weekReport.rangeOffset" type="number" min="-52" max="52" />
          </label>
          <label class="settings-field settings-stack">
            <span>模板</span>
            <textarea v-model="draft.weekReport.template" rows="7" />
          </label>
        </section>

        <section class="settings-section">
          <h3>日历</h3>
          <label class="settings-check">
            <input v-model="draft.calendar.enabled" type="checkbox" />
            <span>启用日历文件同步</span>
          </label>
          <div class="settings-calendar">
            <span class="settings-calendar__path">{{ draft.calendar.path || "尚未选择 .ics 文件" }}</span>
            <div>
              <button type="button" class="btn btn--sm" @click="chooseCalendar">选择文件</button>
              <button
                type="button"
                class="btn btn--sm"
                :disabled="!draft.calendar.path || calendarLoading"
                @click="refreshCalendarFile"
              >
                {{ calendarLoading ? "读取中…" : "刷新" }}
              </button>
              <button type="button" class="btn btn--sm btn--ghost" :disabled="!draft.calendar.path" @click="clearCalendar">移除</button>
            </div>
            <small>{{ draft.calendar.lastStatus || "选择后读取今天起 7 天的日程" }}</small>
          </div>
        </section>

        <section class="settings-section">
          <h3>飞书与企业微信</h3>
          <div class="settings-service">
            <label class="settings-check">
              <input v-model="draft.feishu.enabled" type="checkbox" />
              <span>启用飞书 Webhook</span>
            </label>
            <div class="settings-service__actions">
              <input v-model="draft.feishu.webhook" type="url" placeholder="飞书机器人 Webhook" />
              <button
                type="button"
                class="btn btn--sm"
                :disabled="!draft.feishu.webhook || !!testingWebhook"
                @click="testRobot('feishu')"
              >
                {{ testingWebhook === "feishu" ? "测试中…" : "测试" }}
              </button>
            </div>
            <small>{{ draft.feishu.lastStatus || "尚无发送状态" }}</small>
          </div>
          <div class="settings-service">
            <label class="settings-check">
              <input v-model="draft.wecom.enabled" type="checkbox" />
              <span>启用企业微信 Webhook</span>
            </label>
            <div class="settings-service__actions">
              <input v-model="draft.wecom.webhook" type="url" placeholder="企业微信机器人 Webhook" />
              <button
                type="button"
                class="btn btn--sm"
                :disabled="!draft.wecom.webhook || !!testingWebhook"
                @click="testRobot('wecom')"
              >
                {{ testingWebhook === "wecom" ? "测试中…" : "测试" }}
              </button>
            </div>
            <small>{{ draft.wecom.lastStatus || "尚无发送状态" }}</small>
          </div>
        </section>

        <section class="settings-section">
          <h3>快速记录</h3>
          <label class="settings-check">
            <input v-model="draft.quickCapture.enabled" type="checkbox" />
            <span>启用全局快速记录</span>
          </label>
          <label class="settings-field settings-stack">
            <span>全局快捷键</span>
            <input
              v-model.trim="draft.quickCapture.shortcut"
              type="text"
              placeholder="Alt+Space"
              :disabled="!draft.quickCapture.enabled"
            />
          </label>
          <p
            class="settings-note"
            :class="{ 'settings-error': draft.quickCapture.lastStatus.startsWith('注册失败') || draft.quickCapture.lastStatus.startsWith('注销快捷键失败') }"
          >
            {{ draft.quickCapture.lastStatus || "默认 Alt+Space；若与其他应用冲突，保存后会在这里显示原因。" }}
          </p>
        </section>

        <section class="settings-section">
          <h3>DeepSeek</h3>
          <label class="settings-check">
            <input v-model="draft.deepSeek.enabled" type="checkbox" />
            <span>启用 DeepSeek</span>
          </label>
          <label class="settings-field settings-stack">
            <span>Base URL</span>
            <input v-model="draft.deepSeek.baseUrl" type="url" />
          </label>
          <label class="settings-field settings-stack">
            <span>模型</span>
            <input v-model="draft.deepSeek.model" type="text" />
          </label>
          <label class="settings-field settings-stack">
            <span>API Key（{{ draft.deepSeek.hasApiKey ? "已安全保存" : "未设置" }}）</span>
            <input
              v-model="apiKey"
              type="password"
              autocomplete="new-password"
              :placeholder="draft.deepSeek.hasApiKey ? '••••••••••••••••' : '请输入 API Key'"
            />
          </label>
          <div class="settings-service__actions">
            <button
              type="button"
              class="btn btn--sm"
              :disabled="testingDeepSeek || (!apiKey && !draft.deepSeek.hasApiKey)"
              @click="testAi"
            >
              {{ testingDeepSeek ? "测试中…" : "保存并测试连接" }}
            </button>
          </div>
          <p class="settings-note">{{ draft.deepSeek.lastStatus || "尚未测试连接" }}</p>
          <label v-if="draft.deepSeek.hasApiKey" class="settings-check settings-danger">
            <input v-model="clearApiKey" type="checkbox" />
            <span>删除已保存的 API Key</span>
          </label>
          <p class="settings-note">
            {{ draft.deepSeek.hasApiKey ? "已显示为掩码；输入新值即可替换。" : "请输入 API Key 后保存。" }}
            API Key 由 Windows DPAPI 加密保存，不会随设置读取返回明文。
          </p>
        </section>

        <section class="settings-section">
          <h3>数据导出</h3>
          <p class="settings-note">导出为 Excel 工作簿，分为“待办”和“计划”两个 Sheet，包含已完成记录、计划阶段及时间信息。</p>
          <div class="settings-service__actions settings-export-actions">
            <button
              type="button"
              class="btn btn--sm"
              :disabled="exportingRecords"
              @click="exportAllRecords"
            >
              {{ exportingRecords ? "导出中…" : "导出所有记录" }}
            </button>
          </div>
          <p v-if="exportStatus" class="settings-note">{{ exportStatus }}</p>
        </section>

        <p v-if="externalEnabled" class="settings-consent">
          联网与全局能力首次启用说明：日历路径会访问本机文件；Webhook 与 DeepSeek 会向所填地址发送数据；快速记录会使用全局能力。
        </p>
      </div>

      <footer class="settings-foot">
        <span v-if="error" class="settings-error">{{ error }}</span>
        <span v-else-if="saved" class="settings-saved">设置已保存</span>
        <button type="button" class="btn" @click="emit('close')">取消</button>
        <button type="button" class="btn btn--primary" :disabled="saving" @click="save">
          {{ saving ? "保存中…" : "保存" }}
        </button>
      </footer>
    </aside>
  </div>
</template>
