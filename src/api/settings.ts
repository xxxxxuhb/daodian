import { invoke } from "@tauri-apps/api/core";
import type { Settings, WindowFrame, WindowTier } from "../domain/settings";

export interface LegacySettings {
  theme?: string;
  frames?: Partial<Record<WindowTier, WindowFrame>>;
  weekTemplate?: string;
}

export function getSettings() {
  return invoke<Settings>("get_settings");
}

export function saveSettings(
  settings: Settings,
  deepSeekApiKey?: string,
  clearDeepSeekApiKey = false,
) {
  return invoke<Settings>("save_settings", {
    settings,
    deepSeekApiKey,
    clearDeepSeekApiKey,
  });
}

export function migrateLegacySettings(legacy: LegacySettings) {
  return invoke<Settings>("migrate_legacy_settings", {
    theme: legacy.theme,
    frames: legacy.frames,
    weekTemplate: legacy.weekTemplate,
  });
}

export function testWebhook(provider: "feishu" | "wecom", webhook: string) {
  return invoke<string>("test_webhook", { provider, webhook });
}

export interface BackupInfo {
  id: string;
  label: string;
  files: string[];
  createdAt: number;
}

export function listBackups() {
  return invoke<BackupInfo[]>("list_backups");
}

export function restoreBackup(id: string) {
  return invoke<string>("restore_backup", { id });
}
