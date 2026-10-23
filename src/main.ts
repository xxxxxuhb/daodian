import { createApp } from "vue";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.vue";
import "./features/shell/theme";
import "./styles.css";
import "../design/v2/v2.css";
import "./v2-live.css";

function logFrontend(level: "info" | "warn" | "error", message: string) {
  invoke("frontend_log", { level, message }).catch(() => {});
}

window.addEventListener("error", (event) => {
  const where = event.filename ? ` (${event.filename}:${event.lineno}:${event.colno})` : "";
  logFrontend("error", `${event.message || "window error"}${where}`);
});

window.addEventListener("unhandledrejection", (event) => {
  const reason =
    event.reason instanceof Error
      ? `${event.reason.message}\n${event.reason.stack ?? ""}`
      : String(event.reason);
  logFrontend("error", `unhandled rejection: ${reason}`);
});

logFrontend("info", `Vue bootstrap; url=${location.href}`);

// 免打扰按本地时间算，Rust 侧没有时区信息，启动时同步一次。
invoke("sync_clock", { offsetMinutes: new Date().getTimezoneOffset() }).catch(() => {});

try {
  createApp(App).mount("#app");
  logFrontend("info", "Vue mounted");
} catch (error) {
  const message = error instanceof Error ? `${error.message}\n${error.stack ?? ""}` : String(error);
  logFrontend("error", `Vue mount failed: ${message}`);
  throw error;
}
