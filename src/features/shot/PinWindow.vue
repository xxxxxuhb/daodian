<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { readFile } from "@tauri-apps/plugin-fs";
import { onBeforeUnmount, onMounted, shallowRef, useTemplateRef } from "vue";
import { errorText } from "../../domain/feedback";

const id = Number(new URLSearchParams(location.search).get("id") ?? "0");
const error = shallowRef("");
const canvasRef = useTemplateRef<HTMLCanvasElement>("canvas");

onMounted(() => {
  window.addEventListener("keydown", onKey);
  void load();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
});

function log(level: "info" | "error", message: string) {
  void invoke("frontend_log", { level, message }).catch(() => {});
}

async function load() {
  try {
    const info = await invoke<{ path: string; width: number; height: number }>("shot_pin_info", { id });
    const bytes = new Uint8Array(await readFile(info.path));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = canvasRef.value;
    if (!canvas) throw new Error("贴图画布还没准备好");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("贴图画布不可用");
    ctx.fillStyle = "#101816";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(bitmap, 0, 0);
    const painted = `${bitmap.width}x${bitmap.height}`;
    bitmap.close();
    log("info", `pin image painted: ${id} ${painted} bytes=${bytes.byteLength}`);
  } catch (cause) {
    error.value = errorText(cause);
    log("error", `pin image failed: ${id} ${error.value}`);
  }
}

function onKey(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  event.preventDefault();
  close();
}

function close() {
  void getCurrentWindow().close();
}
</script>

<template>
  <div class="pin">
    <canvas ref="canvas" class="pin-canvas" />
    <div class="pin-drag" data-tauri-drag-region />
    <p v-if="error" class="pin-error">{{ error }}</p>
  </div>
</template>

<style scoped>
.pin {
  position: fixed;
  inset: 0;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: #101816;
}

.pin-canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.pin-drag {
  position: absolute;
  inset: 0;
  z-index: 1;
}

.pin-error {
  position: absolute;
  top: 8px;
  right: 8px;
  left: 8px;
  z-index: 3;
  margin: 0;
  padding: 8px 10px;
  border-radius: 8px;
  background: #3a1414;
  color: #ffd0d0;
  font: 12px/1.4 "Segoe UI", "Microsoft YaHei UI", sans-serif;
}
</style>
