import { convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import {
  beginAttachment,
  commitAttachment,
  finishAttachment,
  importAttachment,
  prepareAttachment,
  pushAttachmentChunk,
} from "../../api/notes";
import type { Attachment } from "../../domain/notes";

const MAX_IMAGE = 10 * 1024 * 1024;

export function mediaUrl(dir: string, name: string) {
  if (!dir || !name) return "";
  const sep = dir.includes("\\") ? "\\" : "/";
  const path = dir.endsWith("\\") || dir.endsWith("/") ? `${dir}${name}` : `${dir}${sep}${name}`;
  try {
    return convertFileSrc(path);
  } catch {
    return "";
  }
}

export function extensionOf(file: File) {
  const fromName = file.name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "webp", "bmp"].includes(fromName)) return fromName === "jpeg" ? "jpg" : fromName;
  if (file.type === "image/jpeg") return "jpg";
  if (file.type === "image/gif") return "gif";
  if (file.type === "image/webp") return "webp";
  if (file.type === "image/bmp") return "bmp";
  return "png";
}

export async function storeImage(file: File): Promise<Attachment> {
  if (file.size > MAX_IMAGE) throw new Error("图片超过 10MB，压缩后再传");
  const ext = extensionOf(file);
  const bytes = new Uint8Array(await file.arrayBuffer());
  try {
    const tmp = await prepareAttachment();
    await writeFile(tmp, bytes);
    return await commitAttachment(tmp, ext);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (message.includes("10MB") || message.includes("只接收") || message.includes("读不了") || message.includes("空的")) {
      throw error;
    }
    return uploadChunks(bytes, ext);
  }
}

export async function pickImage() {
  const selected = await open({
    multiple: false,
    filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
  });
  if (typeof selected !== "string") return null;
  return importAttachment(selected);
}

async function uploadChunks(bytes: Uint8Array, ext: string) {
  const id = await beginAttachment();
  const size = 192 * 1024;
  for (let offset = 0; offset < bytes.length; offset += size) {
    await pushAttachmentChunk(id, toBase64(bytes.subarray(offset, offset + size)));
  }
  return finishAttachment(id, ext);
}

function toBase64(bytes: Uint8Array) {
  let binary = "";
  const step = 0x8000;
  for (let index = 0; index < bytes.length; index += step) {
    binary += String.fromCharCode(...bytes.subarray(index, index + step));
  }
  return btoa(binary);
}
