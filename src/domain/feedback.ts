export function errorText(error: unknown) {
  if (typeof error === "string") return error;
  if (error instanceof Error && error.message) {
    if (error.message.includes("invoke")) return "还没连上桌面端，保存和提醒要在应用窗口里用";
    return error.message;
  }
  return "操作失败";
}
