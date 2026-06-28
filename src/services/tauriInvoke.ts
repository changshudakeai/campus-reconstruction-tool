import { invoke, isTauri } from "@tauri-apps/api/core";

export const DESKTOP_REQUIRED_MESSAGE =
  "此操作需要 Tauri 桌面应用。请双击 start-app.cmd 启动，不要在 http://127.0.0.1:1420 浏览器页面中执行建筑查询或生成。";

export function invokeDesktop<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error(DESKTOP_REQUIRED_MESSAGE));
  return invoke<T>(command, args);
}
