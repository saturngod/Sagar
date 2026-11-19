import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { loadAutoUpdateCheck, saveAutoUpdateCheck } from "../../lib/prefs";

export const RELEASES_URL = "https://github.com/saturngod/Sagar/releases/latest";

/**
 * App update lifecycle. Only the signed `latest.json` manifest and the
 * matching bundle are fetched from GitHub; no book content or user data is
 * sent. The updater plugin verifies the signature before installing.
 */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "downloading"; version: string; received: number; total: number | null }
  | { kind: "ready"; version: string }
  | { kind: "error"; phase: "check" | "install"; message: string };

type UpdateContextValue = {
  state: UpdateState;
  currentVersion: string | null;
  autoCheck: boolean;
  setAutoCheck: (enabled: boolean) => void;
  checkNow: () => Promise<void>;
  install: () => Promise<void>;
  restart: () => Promise<void>;
  openReleasePage: () => void;
  /** Hides the library banner until the next launch or check. */
  dismissed: boolean;
  dismiss: () => void;
};

const UpdateContext = createContext<UpdateContextValue | null>(null);

const AUTO_CHECK_DELAY_MS = 4000;

export function UpdateProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
  const [autoCheck, setAutoCheckState] = useState(loadAutoUpdateCheck);
  const [dismissed, setDismissed] = useState(false);
  const update = useRef<Update | null>(null);
  const busy = useRef(false);

  useEffect(() => {
    getVersion()
      .then(setCurrentVersion)
      .catch(() => setCurrentVersion(null));
  }, []);

  const checkNow = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setDismissed(false);
    setState({ kind: "checking" });
    try {
      const found = await check();
      const previous = update.current;
      update.current = found;
      if (previous && previous !== found) void previous.close().catch(() => {});
      setState(
        found
          ? { kind: "available", version: found.version, notes: found.body ?? null }
          : { kind: "upToDate" },
      );
    } catch {
      setState({
        kind: "error",
        phase: "check",
        message:
          "Couldn’t check for updates. Check your internet connection, then retry.",
      });
    } finally {
      busy.current = false;
    }
  }, []);

  const install = useCallback(async () => {
    const pending = update.current;
    if (!pending || busy.current) return;
    busy.current = true;
    const version = pending.version;
    let received = 0;
    let total: number | null = null;
    setState({ kind: "downloading", version, received, total });
    try {
      await pending.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
        }
        setState({ kind: "downloading", version, received, total });
      });
      setState({ kind: "ready", version });
    } catch {
      setState({
        kind: "error",
        phase: "install",
        message:
          "The update couldn’t be installed. Retry, or download it from the release page.",
      });
    } finally {
      busy.current = false;
    }
  }, []);

  const restart = useCallback(async () => {
    try {
      await relaunch();
    } catch {
      setState({
        kind: "error",
        phase: "install",
        message: "The update is installed. Quit and reopen Sagar to finish.",
      });
    }
  }, []);

  const setAutoCheck = useCallback((enabled: boolean) => {
    setAutoCheckState(enabled);
    saveAutoUpdateCheck(enabled);
  }, []);

  const openReleasePage = useCallback(() => {
    void openUrl(RELEASES_URL).catch(() => {});
  }, []);

  // One quiet check per launch when allowed. Dev builds never self-update.
  const autoChecked = useRef(false);
  useEffect(() => {
    if (!autoCheck || autoChecked.current || import.meta.env.DEV) return;
    autoChecked.current = true;
    const timer = window.setTimeout(() => void checkNow(), AUTO_CHECK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [autoCheck, checkNow]);

  const value = useMemo<UpdateContextValue>(
    () => ({
      state,
      currentVersion,
      autoCheck,
      setAutoCheck,
      checkNow,
      install,
      restart,
      openReleasePage,
      dismissed,
      dismiss: () => setDismissed(true),
    }),
    [
      state,
      currentVersion,
      autoCheck,
      setAutoCheck,
      checkNow,
      install,
      restart,
      openReleasePage,
      dismissed,
    ],
  );

  return <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>;
}

export function useAppUpdate(): UpdateContextValue {
  const value = useContext(UpdateContext);
  if (!value) throw new Error("useAppUpdate must be used inside UpdateProvider");
  return value;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(0, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
