import { invoke } from "@tauri-apps/api/core";
import { toCommandError } from "./errors";

export interface ContentServerConfig {
  /** 0 chooses an available port. */
  port: number;
  username: string;
  password: string;
}
export interface ContentServerStatus {
  running: boolean;
  port: number | null;
  authenticationRequired: boolean;
  urls: string[];
}
export async function startContentServer(
  config: ContentServerConfig,
): Promise<ContentServerStatus> {
  try {
    return await invoke("start_content_server", { config });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function stopContentServer(): Promise<ContentServerStatus> {
  try {
    return await invoke("stop_content_server");
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function contentServerStatus(): Promise<ContentServerStatus> {
  try {
    return await invoke("content_server_status");
  } catch (error) {
    throw toCommandError(error);
  }
}
