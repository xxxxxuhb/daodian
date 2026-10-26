import { ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { getSettings, migrateLegacySettings, saveSettings } from "../api/settings";
import { DEFAULT_WEEK_REPORT_TEMPLATE } from "../domain/present";
import type { Settings, WindowFrame, WindowTier } from "../domain/settings";

const settings = ref<Settings>(fallbackSettings());
const loading = ref(true);
const saving = ref(false);
const error = ref("");
let loadPromise: Promise<void> | null = null;
let saveQueue: Promise<unknown> = Promise.resolve();
let statusListener: Promise<unknown> | null = null;

interface WebhookStatusEvent {
  provider: "feishu" | "wecom";
  status: string;
}

function fallbackSettings(): Settings {
  return {
    theme: readLocal("daodian-theme") === "night" ? "night" : "celadon",
    launchAtStartup: false,
    window: { frames: readFrames(), lastTier: "", defaultTier: "standard" },
    pet: { enabled: true, pinches: 0, hiddenUntil: 0, idleMinSeconds: 12, idleMaxSeconds: 28 },
    repeat: { defaultSkipWeekends: true, holidays: [], workdays: [] },
    workHours: {
      morningStart: "09:00",
      morningEnd: "12:00",
      afternoonStart: "13:30",
      afternoonEnd: "18:00",
    },
    weekReport: {
      template: DEFAULT_WEEK_REPORT_TEMPLATE,
      rangeOffset: 0,
    },
    calendar: { enabled: false, path: "", lastStatus: "" },
    feishu: { enabled: false, webhook: "", lastStatus: "" },
    wecom: { enabled: false, webhook: "", lastStatus: "" },
    quickCapture: { enabled: false, shortcut: "Alt+Space", lastStatus: "" },
    deepSeek: {
      enabled: false,
      baseUrl: "https://api.deepseek.com",
      model: "deepseek-chat",
      hasApiKey: false,
      lastStatus: "",
    },
  };
}

function readLocal(key: string) {
  try {
    return localStorage.getItem(key) || "";
  } catch {
    return "";
  }
}

function readFrames(): Partial<Record<WindowTier, WindowFrame>> {
  try {
    return JSON.parse(readLocal("daodian-frames") || "{}") as Partial<
      Record<WindowTier, WindowFrame>
    >;
  } catch {
    return {};
  }
}

async function load() {
  statusListener ??= listen<WebhookStatusEvent>("webhook-status", ({ payload }) => {
    settings.value[payload.provider].lastStatus = payload.status;
  }).catch(() => undefined);
  if (loadPromise) return loadPromise;
  loadPromise = (async () => {
    try {
      const loaded = await getSettings();
      settings.value = await migrateLegacySettings({
        theme: readLocal("daodian-theme") || undefined,
        frames: readFrames(),
        weekTemplate: readLocal("daodian-week-report-template") || undefined,
      }).catch(() => loaded);
      error.value = "";
    } catch (cause) {
      // 浏览器预览和旧版后端继续使用 localStorage 兼容值。
      error.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading.value = false;
    }
  })();
  return loadPromise;
}

function update(
  change: (draft: Settings) => void,
  secret?: { apiKey?: string; clearApiKey?: boolean },
) {
  const next = JSON.parse(JSON.stringify(settings.value)) as Settings;
  change(next);
  settings.value = next;
  saving.value = true;
  saveQueue = saveQueue
    .catch(() => undefined)
    .then(async () => {
      try {
        settings.value = await saveSettings(next, secret?.apiKey, secret?.clearApiKey);
        error.value = "";
      } catch (cause) {
        // 设置已在本次会话生效；不把未接入能力伪报为执行成功。
        error.value = cause instanceof Error ? cause.message : String(cause);
        throw cause;
      }
    })
    .finally(() => {
      saving.value = false;
    });
  return saveQueue;
}

export function useSettings() {
  return { settings, loading, saving, error, load, update };
}
