<script setup lang="ts">
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { downloadDir, join } from "@tauri-apps/api/path";
import { computed, ref, watch } from "vue";
import { utils as xlsxUtils, write as writeWorkbook } from "xlsx";
import { testDeepSeek } from "../../api/ai";
import { createCloudRepo, disconnectCloud, openCloudTokenPage, saveCloudConfig, setCloudToken, testCloud } from "../../api/cloud";
import { loadLibrary } from "../../api/library";
import {
  listBackups,
  restoreBackup,
  testWebhook,
  type BackupInfo,
} from "../../api/settings";
import { useCalendar } from "../../data/useCalendar";
import { cloudStatusLabel, cloudSyncAgo, useCloud } from "../../data/useCloud";
import { useSettings } from "../../data/useSettings";
import { errorText } from "../../domain/feedback";
import { repeatLabel } from "../../domain/repeat";
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
const { status: cloudStatus, syncNow } = useCloud();
const cloudToken = ref("");
const cloudRepoName = ref("daodian-sync");
const cloudBusy = ref("");
const cloudNote = ref("");
const cloudPane = ref<"off" | "connect" | "existing" | "on">("off");
const cloudAdvanced = ref(false);
const cloudReplaceToken = ref(false);
const cloudToast = ref("");
const cloudHadLink = ref(false);
const exportingRecords = ref(false);
const exportStatus = ref("");
const backups = ref<BackupInfo[]>([]);
const selectedBackup = ref("");
const backupLoading = ref(false);
const backupRestoring = ref(false);
const backupStatus = ref("");
const externalEnabled = computed(
  () =>
    draft.value.calendar.enabled ||
    draft.value.feishu.enabled ||
    draft.value.wecom.enabled ||
    draft.value.quickCapture.enabled ||
    draft.value.deepSeek.enabled ||
    draft.value.cloud.enabled,
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
    backupStatus.value = "";
    resetCloudPane();
    void loadBackupHistory();
  },
  { immediate: true },
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

const cloudStateKind = computed(() => {
  if (cloudBusy.value === "sync" || cloudStatus.value.lastStatus === "syncing") return "syncing";
  const code = cloudStatus.value.lastStatus || draft.value.cloud.lastStatus;
  if (code === "ok") return "ok";
  if (code === "conflict" || code.startsWith("error")) return "bad";
  return "off";
});
const cloudStateText = computed(() => {
  if (!draft.value.cloud.enabled) return "已关闭，不会上传";
  if (cloudStateKind.value === "syncing") return "正在同步";
  const code = cloudStatus.value.lastStatus || draft.value.cloud.lastStatus;
  const at = cloudStatus.value.lastSyncAt || draft.value.cloud.lastSyncAt;
  const message = cloudStatus.value.message;
  const phrase =
    code === "ok"
      ? "正常"
      : code === "conflict"
        ? "有冲突，已保留本机"
        : code.startsWith("error")
          ? message && message !== "云同步还没打开"
            ? message
            : cloudStatusLabel(code)
          : "尚未同步";
  if (!at) return cloudStateKind.value === "bad" ? phrase : "还没有同步过";
  return `上次同步 ${cloudSyncAgo(at)} · ${phrase}`;
});

function resetCloudPane() {
  cloudPane.value = draft.value.cloud.owner.trim() && draft.value.cloud.repo.trim() ? "on" : "off";
  cloudAdvanced.value = false;
  cloudReplaceToken.value = false;
  cloudToast.value = "";
  cloudNote.value = "";
  cloudToken.value = "";
  cloudHadLink.value = false;
}

function showExistingRepo() {
  cloudHadLink.value = Boolean(draft.value.cloud.owner.trim() && draft.value.cloud.repo.trim());
  cloudPane.value = "existing";
}

async function rememberCloudConfig() {
  const saved = await saveCloudConfig(draft.value.cloud);
  draft.value.cloud.hasToken = saved.hasToken;
  draft.value.cloud.lastStatus = saved.lastStatus;
  draft.value.cloud.lastSyncAt = saved.lastSyncAt;
  draft.value.cloud.deviceId = saved.deviceId;
}

async function keepTypedToken() {
  if (!cloudToken.value.trim()) return;
  await setCloudToken(cloudToken.value.trim());
  cloudToken.value = "";
  draft.value.cloud.hasToken = true;
}

async function createCloud() {
  cloudBusy.value = "create";
  cloudNote.value = "";
  try {
    await keepTypedToken();
    const info = await createCloudRepo(cloudRepoName.value);
    draft.value.cloud.enabled = true;
    draft.value.cloud.owner = info.owner;
    draft.value.cloud.repo = info.repo;
    draft.value.cloud.branch = info.branch;
    draft.value.cloud.hasToken = true;
    cloudToast.value = `已连接到 ${info.owner}/${info.repo}`;
    cloudPane.value = "on";
    cloudAdvanced.value = false;
  } catch (cause) {
    cloudNote.value = errorText(cause);
  } finally {
    cloudBusy.value = "";
  }
}

async function connectExisting() {
  cloudBusy.value = "connect";
  cloudNote.value = "";
  try {
    if (!draft.value.cloud.owner.trim() || !draft.value.cloud.repo.trim()) {
      cloudNote.value = "先填写仓库的所有者和名字";
      return;
    }
    await keepTypedToken();
    draft.value.cloud.enabled = true;
    if (!draft.value.cloud.dir.trim()) draft.value.cloud.dir = "daodian";
    await rememberCloudConfig();
    await testCloud();
    cloudToast.value = `已连接到 ${draft.value.cloud.owner}/${draft.value.cloud.repo}`;
    cloudPane.value = "on";
    cloudAdvanced.value = false;
    await syncNow();
  } catch (cause) {
    cloudNote.value = errorText(cause);
  } finally {
    cloudBusy.value = "";
  }
}

async function toggleCloudEnabled() {
  draft.value.cloud.enabled = !draft.value.cloud.enabled;
  try {
    await rememberCloudConfig();
  } catch (cause) {
    draft.value.cloud.enabled = !draft.value.cloud.enabled;
    cloudNote.value = errorText(cause);
  }
}

async function toggleCloudAuto() {
  draft.value.cloud.autoSync = !draft.value.cloud.autoSync;
  try {
    await rememberCloudConfig();
  } catch (cause) {
    draft.value.cloud.autoSync = !draft.value.cloud.autoSync;
    cloudNote.value = errorText(cause);
  }
}

async function saveReplacementToken() {
  cloudBusy.value = "token";
  cloudNote.value = "";
  try {
    await keepTypedToken();
    cloudReplaceToken.value = false;
    cloudNote.value = "令牌已更换";
  } catch (cause) {
    cloudNote.value = errorText(cause);
  } finally {
    cloudBusy.value = "";
  }
}

async function syncCloud() {
  cloudBusy.value = "sync";
  cloudNote.value = "";
  try {
    await rememberCloudConfig();
    await syncNow();
  } catch (cause) {
    cloudNote.value = errorText(cause);
  } finally {
    cloudBusy.value = "";
  }
}

async function disconnectCloudLink() {
  cloudBusy.value = "disconnect";
  cloudNote.value = "";
  try {
    await disconnectCloud();
    draft.value.cloud.enabled = false;
    draft.value.cloud.owner = "";
    draft.value.cloud.repo = "";
    draft.value.cloud.branch = "";
    draft.value.cloud.lastStatus = "idle";
    draft.value.cloud.lastSyncAt = 0;
    await rememberCloudConfig();
    cloudToast.value = "";
    cloudAdvanced.value = false;
    cloudReplaceToken.value = false;
    cloudPane.value = "off";
  } catch (cause) {
    cloudNote.value = errorText(cause);
  } finally {
    cloudBusy.value = "";
  }
}

function openTokenHelp() {
  void openCloudTokenPage().catch((cause) => {
    cloudNote.value = errorText(cause);
  });
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
      重复规则: repeatLabel(todo.repeat, todo.dueAt),
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

async function loadBackupHistory() {
  backupLoading.value = true;
  try {
    backups.value = await listBackups();
    if (!backups.value.some((item) => item.id === selectedBackup.value)) {
      selectedBackup.value = backups.value[0]?.id ?? "";
    }
  } catch (cause) {
    backupStatus.value = `读取备份失败：${cause instanceof Error ? cause.message : String(cause)}`;
  } finally {
    backupLoading.value = false;
  }
}

async function restoreSelectedBackup() {
  const backup = backups.value.find((item) => item.id === selectedBackup.value);
  if (!backup) return;
  const confirmed = window.confirm(
    `确定恢复“${backup.label}”吗？\n\n当前数据会先自动备份，恢复后应用将刷新。`,
  );
  if (!confirmed) return;

  backupRestoring.value = true;
  backupStatus.value = "";
  try {
    backupStatus.value = await restoreBackup(backup.id);
    window.location.reload();
  } catch (cause) {
    backupStatus.value = `恢复失败：${cause instanceof Error ? cause.message : String(cause)}`;
    backupRestoring.value = false;
  }
}

function displayTime(value: number | null) {
  return value == null ? "" : new Date(value).toLocaleString("zh-CN", { hour12: false });
}

function priorityLabel(value: "low" | "normal" | "high") {
  return value === "high" ? "重要" : value === "low" ? "可缓" : "普通";
}

function displayWidth(value: string) {
  return [...value].reduce(
    (width, char) => width + (char.charCodeAt(0) > 255 ? 2 : 1),
    0,
  );
}

function sheetWithWidths(rows: Record<string, unknown>[], headers: string[]) {
  const sheet = xlsxUtils.json_to_sheet(rows, { header: headers });
  sheet["!cols"] = headers.map((header) => {
    const width = Math.max(
      displayWidth(header) + 4,
      ...rows.map((row) => displayWidth(String(row[header] ?? "")) + 4),
    );
    return { wch: Math.min(48, Math.max(14, width)) };
  });
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
          <label class="settings-field">
            <span>默认窗口大小</span>
            <select v-model="draft.window.defaultTier">
              <option value="mini">小窗口</option>
              <option value="standard">中窗口</option>
              <option value="wide">大窗口</option>
            </select>
          </label>
          <label class="settings-check">
            <input v-model="draft.launchAtStartup" type="checkbox" />
            <span>开机自动启动（启动后隐藏到托盘）</span>
          </label>
          <p class="settings-note">
            保存后，下次启动应用会以所选大小打开；每档窗口会分别记住位置。
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
          <p class="settings-note" v-pre>
            可用变量：{{overview}}、{{todos}}、{{plans}}、{{completed}}、{{activePlans}}、{{nextWeek}}
          </p>
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
          <h3>桌面珠子</h3>
          <label class="settings-check">
            <input v-model="draft.pet.enabled" type="checkbox" />
            <span>在桌面显示陪伴珠子</span>
          </label>
          <label class="settings-field">
            <span>最短间隔（秒）</span>
            <input v-model.number="draft.pet.idleMinSeconds" type="number" min="5" max="1800" step="1" />
          </label>
          <label class="settings-field">
            <span>最长间隔（秒）</span>
            <input
              v-model.number="draft.pet.idleMaxSeconds"
              type="number"
              :min="Math.max(5, draft.pet.idleMinSeconds || 5)"
              max="1800"
              step="1"
            />
          </label>
          <p class="settings-note">
            空闲时会在最短和最长间隔之间随机做一个小动作。最短不能小于 5 秒。
          </p>
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
          <h3>云同步</h3>

          <div v-if="cloudPane === 'off'">
            <p class="cloud-lead">把待办、计划、模板和备忘录同步到你的 Gitee 私有仓库。<b>本地始终是唯一记录</b>，云端只是副本。</p>
            <button type="button" class="cloud-go" :disabled="cloudBusy !== ''" @click="cloudPane = 'connect'">连接 Gitee</button>
            <p class="cloud-links">需要一个访问令牌 · <button type="button" class="cloud-link" @click="openTokenHelp">怎么获取</button></p>
            <p class="cloud-links">令牌只在这台机器上加密保存，不上传、不写进配置文件。</p>
          </div>

          <div v-else-if="cloudPane === 'connect'">
            <p class="cloud-lead">粘贴 Gitee 访问令牌，剩下的我来：验证、建私有仓库、连上并推一次。</p>
            <label class="cloud-field">
              <span>访问令牌</span>
              <input v-model="cloudToken" type="password" autocomplete="new-password" :placeholder="draft.cloud.hasToken ? '已保存，留空则继续用它' : '粘贴私人令牌'" />
            </label>
            <p class="cloud-links"><button type="button" class="cloud-link" @click="openTokenHelp">去 Gitee 生成令牌 ↗</button></p>
            <button type="button" class="cloud-go" :disabled="cloudBusy !== '' || (!cloudToken.trim() && !draft.cloud.hasToken)" @click="createCloud">
              {{ cloudBusy === "create" ? "连接中…" : "连接并创建仓库" }}
            </button>
            <div class="cloud-steps">
              <div class="cloud-step"><i>1</i><span>验证令牌 · 拿到你的登录名</span></div>
              <div class="cloud-step">
                <i>2</i>
                <span>建一个私有仓库 <input v-model.trim="cloudRepoName" class="cloud-name" type="text" aria-label="仓库名" />（名字可以改）</span>
              </div>
              <div class="cloud-step"><i>3</i><span>把这台机器的数据推上去</span></div>
            </div>
            <p class="cloud-links">
              <button v-if="cloudHadLink" type="button" class="cloud-link" @click="cloudPane = 'on'">返回</button>
              <button type="button" class="cloud-link" @click="showExistingRepo">改用已有仓库</button>
            </p>
          </div>

          <div v-else-if="cloudPane === 'existing'">
            <p class="cloud-lead">填一个你已有的私有仓库。公开仓库不会用来同步。</p>
            <label v-if="!draft.cloud.hasToken" class="cloud-field">
              <span>访问令牌</span>
              <input v-model="cloudToken" type="password" autocomplete="new-password" placeholder="粘贴私人令牌" />
            </label>
            <label class="cloud-field">
              <span>仓库所有者 / 仓库名</span>
              <span class="cloud-pair">
                <input v-model.trim="draft.cloud.owner" type="text" placeholder="owner" />
                <input v-model.trim="draft.cloud.repo" type="text" placeholder="repo" />
              </span>
            </label>
            <label class="cloud-field">
              <span>分支 / 目录</span>
              <span class="cloud-pair">
                <input v-model.trim="draft.cloud.branch" type="text" placeholder="留空用默认分支" />
                <input v-model.trim="draft.cloud.dir" type="text" placeholder="daodian" />
              </span>
            </label>
            <button type="button" class="cloud-go" :disabled="cloudBusy !== '' || (!cloudToken.trim() && !draft.cloud.hasToken)" @click="connectExisting">
              {{ cloudBusy === "connect" ? "连接中…" : "连接这个仓库" }}
            </button>
            <p class="cloud-links">
              <button v-if="cloudHadLink" type="button" class="cloud-link" @click="cloudPane = 'on'">返回</button>
              <button type="button" class="cloud-link" @click="cloudPane = 'connect'">改为新建仓库</button>
            </p>
          </div>

          <div v-else>
            <div class="cloud-head">
              <button type="button" class="cloud-sw" :class="{ on: draft.cloud.enabled }" role="switch" :aria-checked="draft.cloud.enabled" @click="toggleCloudEnabled">
                <i></i><span>{{ draft.cloud.enabled ? "已开启" : "已关闭" }}</span>
              </button>
              <button type="button" class="btn btn--sm" :disabled="!draft.cloud.enabled || cloudBusy !== ''" @click="syncCloud">
                {{ cloudBusy === "sync" ? "同步中…" : "立即同步" }}
              </button>
            </div>
            <p class="cloud-repo"><b>{{ draft.cloud.owner }}/{{ draft.cloud.repo }}</b><span>· {{ draft.cloud.branch || "默认分支" }}</span></p>
            <p class="cloud-state" :class="cloudStateKind"><i></i><span>{{ cloudStateText }}</span></p>
            <div class="cloud-adv">
              <button type="button" class="cloud-adv__t" @click="cloudAdvanced = !cloudAdvanced">
                高级 <span>{{ cloudAdvanced ? "▴" : "▾" }}</span>
              </button>
              <div v-if="cloudAdvanced" class="cloud-adv__body">
                <div class="cloud-head">
                  <button type="button" class="cloud-sw" :class="{ on: draft.cloud.autoSync }" role="switch" :aria-checked="draft.cloud.autoSync" :disabled="!draft.cloud.enabled" @click="toggleCloudAuto">
                    <i></i><span>自动同步</span>
                  </button>
                </div>
                <p class="cloud-links">上班 {{ draft.workHours.morningStart }} 和下班 {{ draft.workHours.afternoonEnd }} 各同步一次，其他时间请手动同步。</p>
                <p class="cloud-kv"><span>目录</span><b>{{ draft.cloud.dir || "daodian" }}</b></p>
                <p class="cloud-kv"><span>分支</span><b>{{ draft.cloud.branch || "默认" }}</b><span>· 跟随仓库默认</span></p>
                <div class="cloud-actions">
                  <button type="button" class="btn btn--sm" @click="cloudReplaceToken = !cloudReplaceToken">更换令牌</button>
                  <button type="button" class="btn btn--sm" @click="showExistingRepo">改用已有仓库</button>
                  <button type="button" class="btn btn--sm cloud-stop" :disabled="cloudBusy !== ''" @click="disconnectCloudLink">断开连接</button>
                </div>
                <label v-if="cloudReplaceToken" class="cloud-field">
                  <span>新的访问令牌</span>
                  <span class="cloud-token-row">
                    <input v-model="cloudToken" type="password" autocomplete="new-password" placeholder="粘贴新令牌" />
                    <button type="button" class="btn btn--sm" :disabled="cloudBusy !== '' || !cloudToken.trim()" @click="saveReplacementToken">保存</button>
                  </span>
                </label>
                <p class="cloud-links">断开后本地数据不受影响，只是不再上传。</p>
              </div>
            </div>
            <p v-if="cloudToast" class="cloud-toast"><span>{{ cloudToast }}</span><button type="button" @click="disconnectCloudLink">撤销</button></p>
          </div>
          <p v-if="cloudNote" class="settings-note">{{ cloudNote }}</p>
        </section>

        <section class="settings-section">
          <h3>数据备份与恢复</h3>
          <p class="settings-note">
            应用会在每天进入所设工作时间后，自动备份一次前一天的数据。备份包含待办、计划、模板和设置。
          </p>
          <label class="settings-field settings-stack">
            <span>选择备份</span>
            <select v-model="selectedBackup" :disabled="backupLoading || backupRestoring">
              <option v-if="!backups.length" value="">
                {{ backupLoading ? "正在读取备份…" : "暂无可恢复的备份" }}
              </option>
              <option v-for="backup in backups" :key="backup.id" :value="backup.id">
                {{ backup.label }} · {{ backup.files.length }} 个文件
              </option>
            </select>
          </label>
          <div class="settings-service__actions">
            <button
              type="button"
              class="btn btn--sm"
              :disabled="!selectedBackup || backupLoading || backupRestoring"
              @click="restoreSelectedBackup"
            >
              {{ backupRestoring ? "恢复中…" : "恢复所选备份" }}
            </button>
          </div>
          <p v-if="backupStatus" class="settings-note">{{ backupStatus }}</p>
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
          联网与全局能力首次启用说明：日历路径会访问本机文件；Webhook、DeepSeek 和云同步会向所填地址发送数据；快速记录会使用全局能力。
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
