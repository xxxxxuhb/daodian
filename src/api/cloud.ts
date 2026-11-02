import { invoke } from "@tauri-apps/api/core";
import type { CloudSettings } from "../domain/settings";

export interface CloudStatus extends CloudSettings {
  message: string;
}

export interface CloudReport {
  pulled: boolean;
  pushed: boolean;
  conflict: boolean;
  message: string;
}

export interface CloudRepo {
  owner: string;
  repo: string;
  branch: string;
  created: boolean;
}

export function cloudStatus() {
  return invoke<CloudStatus>("cloud_status");
}

export function saveCloudConfig(cloud: CloudSettings) {
  return invoke<CloudStatus>("cloud_save_config", { cloud });
}

export function setCloudToken(token: string) {
  return invoke<CloudStatus>("cloud_set_token", { token });
}

export function clearCloudToken() {
  return invoke<CloudStatus>("cloud_clear_token");
}

export function createCloudRepo(name: string) {
  return invoke<CloudRepo>("cloud_create_repo", { name });
}

export function testCloud() {
  return invoke<string>("cloud_test");
}

export function syncCloud() {
  return invoke<CloudReport>("cloud_sync_now");
}

export function disconnectCloud() {
  return invoke<CloudStatus>("cloud_disconnect");
}

export function openCloudTokenPage() {
  return invoke<void>("cloud_open_token_page");
}
