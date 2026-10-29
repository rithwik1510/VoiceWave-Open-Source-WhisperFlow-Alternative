import { useState } from "react";
import { exportSupportZip, revealSupportFile } from "../lib/tauri";

/** "Save support file" (plan 024 section 8.2). One per app, so every place
 * that offers it (Settings → Diagnostics, "Something wrong?", the unclean
 * exit banner) shows the same saved path. */
export type SupportFileState =
  | { phase: "idle" | "saving" | "failed" }
  | { phase: "saved"; path: string };

export function useSupportFile() {
  const [state, setState] = useState<SupportFileState>({ phase: "idle" });
  const save = () => {
    setState({ phase: "saving" });
    exportSupportZip().then(
      (path) => setState({ phase: "saved", path }),
      () => setState({ phase: "failed" })
    );
  };
  return { state, save };
}

export type SupportFile = ReturnType<typeof useSupportFile>;

export function SaveSupportFileButton({
  file,
  className = "vw-btn-secondary vw-btn-sm"
}: {
  file: SupportFile;
  className?: string;
}) {
  const saving = file.state.phase === "saving";
  return (
    <button type="button" className={className} onClick={file.save} disabled={saving}>
      {saving ? "Saving…" : "Save support file"}
    </button>
  );
}

/** The line under the button: where it was saved, or that it failed. */
export function SupportFileStatus({ state }: { state: SupportFileState }) {
  if (state.phase === "saved") {
    return (
      <p className="mt-2 text-xs text-faint">
        Saved to <span className="break-all font-mono">{state.path}</span>{" "}
        <button
          type="button"
          className="font-medium text-accent-text"
          onClick={() => void revealSupportFile(state.path).catch(() => undefined)}
        >
          Show in folder
        </button>
      </p>
    );
  }
  if (state.phase === "failed") {
    return (
      <p className="mt-2 text-xs text-status-danger-text">
        Couldn't save the support file. You can still open the logs folder.
      </p>
    );
  }
  return null;
}
