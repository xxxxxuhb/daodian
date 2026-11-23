import { createApp } from "vue";
import { invoke } from "@tauri-apps/api/core";
import ShotOverlay from "./features/shot/ShotOverlay.vue";

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

logFrontend("info", `shot bootstrap; url=${location.href}`);

try {
  createApp(ShotOverlay).mount("#app");
} catch (error) {
  const message = error instanceof Error ? `${error.message}\n${error.stack ?? ""}` : String(error);
  logFrontend("error", `shot mount failed: ${message}`);
  throw error;
}
