import { open } from "@tauri-apps/plugin-dialog";
import { isTauri } from "@tauri-apps/api/core";
import { DESKTOP_REQUIRED_MESSAGE, invokeDesktop } from "./tauriInvoke";

export interface DesktopExportFile {
  fileName: string;
  bytes: Uint8Array;
}

export interface SavedExportBundle {
  directory: string;
  paths: string[];
}

export async function saveExportBundleToChosenFolder(
  files: DesktopExportFile[]
): Promise<SavedExportBundle | null> {
  if (!isTauri()) throw new Error(DESKTOP_REQUIRED_MESSAGE);
  const directory = await open({
    directory: true,
    multiple: false,
    title: "选择 Minecraft 复刻文件的保存文件夹"
  });
  if (!directory || Array.isArray(directory)) return null;
  return invokeDesktop<SavedExportBundle>("save_export_bundle", {
    directory,
    files: files.map((file) => ({ fileName: file.fileName, bytes: Array.from(file.bytes) }))
  });
}

export function utf8Bytes(value: string) {
  return new TextEncoder().encode(value);
}
