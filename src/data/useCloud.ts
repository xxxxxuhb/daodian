import { listen } from "@tauri-apps/api/event";
import { ref } from "vue";
import { cloudStatus, syncCloud, type CloudStatus } from "../api/cloud";
import { errorText } from "../domain/feedback";

const status = ref<CloudStatus>({
  enabled: false,
  provider: "gitee",
  owner: "",
  repo: "",
  branch: "",
  dir: "daodian",
  autoSync: true,
  intervalMinutes: 10,
  deviceId: "",
  lastSyncAt: 0,
  lastStatus: "idle",
  hasToken: false,
  message: "",
});
let listening = false;

export function cloudStatusLabel(code: string) {
  if (code === "syncing") return "同步中";
  if (code === "ok") return "成功";
  if (code === "conflict") return "有冲突，已保留本机内容";
  if (code === "error:offline") return "网络不可用";
  if (code === "error:auth") return "令牌无效或权限不足";
  if (code === "error:repo") return "找不到仓库";
  if (code.startsWith("error:")) return "同步失败";
  return "尚未同步";
}

export function cloudSyncAgo(at: number) {
  if (!at) return "";
  const minutes = Math.max(0, Math.round((Date.now() - at) / 60000));
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return `${Math.round(hours / 24)} 天前`;
}

export function useCloud() {
  if (!listening) {
    listening = true;
    void listen<CloudStatus>("cloud-status", ({ payload }) => {
      status.value = payload;
    }).catch(() => undefined);
    void cloudStatus()
      .then((value) => {
        status.value = value;
      })
      .catch(() => undefined);
  }

  async function syncNow() {
    try {
      const report = await syncCloud();
      status.value = {
        ...status.value,
        lastStatus: report.conflict ? "conflict" : "ok",
        lastSyncAt: Date.now(),
        message: report.message,
      };
      return report.message;
    } catch (error) {
      const message = errorText(error);
      status.value = { ...status.value, lastStatus: "error:gitee", message };
      throw error;
    }
  }

  return { status, syncNow };
}
