import { useEffect, useRef, useState } from "react";
import { ChevronRight, CheckCircle2, Folder, FolderPlus, Home, Users } from "lucide-react";
import { errorMessage, files } from "../../lib/ipc";
import { bytes } from "../../lib/format";
import { Spinner } from "../../lib/Spinner";
import type { FilesEntry } from "../../lib/types";
import { displayPath, lastDir, rememberDir, useFiles, type SaveRequest } from "./store";

type Phase =
  | { kind: "pick" }
  | { kind: "uploading"; sent: number; total: number }
  | { kind: "nameTaken"; name: string; uploadId: string | null }
  | { kind: "done"; path: string };

function parentOf(path: string) {
  const i = path.lastIndexOf("/");
  return i <= 0 ? "/" : path.slice(0, i);
}

let seq = 0;

/** Mount once near the app root; opened with `saveToFiles(...)`. */
export function SaveToFilesHost() {
  const saving = useFiles((s) => s.saving);
  const close = useFiles((s) => s.close);
  if (!saving) return null;
  return <SaveDialog key={saving.id} req={saving} onClose={close} />;
}

function SaveDialog({ req, onClose }: { req: SaveRequest; onClose: () => void }) {
  const status = useFiles((s) => s.status);
  const [dir, setDir] = useState(lastDir());
  const [entries, setEntries] = useState<FilesEntry[] | null>(null);
  const [name, setName] = useState(req.name);
  const [phase, setPhase] = useState<Phase>({ kind: "pick" });
  const [err, setErr] = useState<string | null>(null);
  const [newFolder, setNewFolder] = useState<string | null>(null);
  const key = useRef(`save-${++seq}`).current;

  useEffect(() => {
    let live = true;
    setEntries(null);
    files
      .list(dir)
      .then((e) => live && setEntries(e.filter((x) => x.isDir).sort((a, b) => a.name.localeCompare(b.name))))
      .catch((e) => {
        if (!live) return;
        // A remembered folder may have been deleted; fall back to the top.
        if (dir !== "/") setDir("/");
        else setErr(errorMessage(e));
      });
    return () => {
      live = false;
    };
  }, [dir]);

  useEffect(() => {
    const un = files.onUpload((p) => {
      if (p.key === key) setPhase({ kind: "uploading", sent: p.sent, total: p.total });
    });
    return () => {
      void un.then((f) => f());
    };
  }, [key]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && phase.kind !== "uploading" && cancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const busy = phase.kind === "uploading";
  const canSave = dir !== "/" && name.trim() !== "" && !busy;

  const save = async () => {
    setErr(null);
    setPhase({ kind: "uploading", sent: 0, total: 0 });
    try {
      const out =
        phase.kind === "nameTaken" && phase.uploadId
          ? await files.renameUpload(phase.uploadId, name.trim())
          : await files.upload(key, req.source, dir, name.trim());
      if (out.kind === "done") {
        rememberDir(dir);
        setPhase({ kind: "done", path: out.path });
      } else {
        setName(out.suggested || name);
        setPhase({ kind: "nameTaken", name: out.name, uploadId: out.uploadId });
      }
    } catch (e) {
      setErr(errorMessage(e));
      setPhase(phase.kind === "nameTaken" ? phase : { kind: "pick" });
    }
  };

  const cancel = () => {
    if (phase.kind === "nameTaken" && phase.uploadId) void files.cancelUpload(phase.uploadId).catch(() => {});
    onClose();
  };

  const createFolder = async () => {
    if (!newFolder?.trim()) return;
    try {
      const path = await files.mkdir(dir, newFolder.trim());
      setNewFolder(null);
      setDir(path);
    } catch (e) {
      setErr(errorMessage(e));
    }
  };

  const crumbs = dir.split("/").filter(Boolean);

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4" onClick={() => !busy && cancel()}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="save-files-title"
        className="flex max-h-[85vh] w-[460px] flex-col rounded-lg border border-gray-300 bg-white p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div id="save-files-title" className="font-medium">
          Save to Trakzen Files
        </div>
        <p className="mt-0.5 truncate text-xs text-gray-500">
          {status?.username ? `as ${status.username} · ` : ""}
          {status?.url}
        </p>

        {phase.kind === "done" ? (
          <div className="mt-5 flex flex-col items-center gap-2 py-4 text-center">
            <CheckCircle2 className="size-10 text-green-600" />
            <p className="text-sm">
              Saved as <b>“{phase.path.split("/").pop()}”</b> in <b>{displayPath(parentOf(phase.path))}</b>
            </p>
            <div className="mt-3 flex gap-2">
              <button className="btn text-sm" onClick={() => void files.open(parentOf(phase.path)).catch((e) => setErr(errorMessage(e)))}>
                Open folder in browser
              </button>
              <button className="btn btn-primary text-sm" onClick={onClose} autoFocus>
                Done
              </button>
            </div>
          </div>
        ) : (
          <>
            <label className="label mt-4">File name</label>
            <input className="input w-full" value={name} onChange={(e) => setName(e.target.value)} disabled={busy} />
            {phase.kind === "nameTaken" && (
              <p className="mt-2 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900">
                <b>“{phase.name}”</b> is already in {displayPath(dir)}. Existing files are never replaced; save it under the suggested name,
                choose another, or cancel.
              </p>
            )}

            <label className="label mt-4">Folder</label>
            <div className="mb-1 flex min-w-0 items-center gap-1 text-sm">
              <button className="text-blue-700 hover:underline disabled:text-gray-400 disabled:no-underline" disabled={busy || dir === "/"} onClick={() => setDir("/")}>
                Trakzen Files
              </button>
              {crumbs.map((c, i) => (
                <span key={i} className="flex min-w-0 items-center gap-1">
                  <ChevronRight className="size-3.5 shrink-0 text-gray-400" />
                  <button
                    className="truncate text-blue-700 hover:underline disabled:text-gray-900 disabled:no-underline"
                    disabled={busy || i === crumbs.length - 1}
                    onClick={() => setDir(`/${crumbs.slice(0, i + 1).join("/")}`)}
                  >
                    {i === 0 ? displayPath(`/${c}`) : c}
                  </button>
                </span>
              ))}
            </div>
            <div className="min-h-40 flex-1 overflow-y-auto rounded-md border border-gray-200">
              {entries === null ? (
                <div className="flex justify-center p-6">
                  <Spinner />
                </div>
              ) : entries.length === 0 ? (
                <p className="p-3 text-sm text-gray-500">No folders here. Save here, or create one.</p>
              ) : (
                entries.map((e) => (
                  <button
                    key={e.path}
                    disabled={busy}
                    onClick={() => setDir(e.path)}
                    className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-gray-100"
                  >
                    {e.kind === "space" ? (
                      e.path === "/home" ? <Home className="size-4 text-blue-600" /> : <Users className="size-4 text-blue-600" />
                    ) : (
                      <Folder className="size-4 text-blue-600" />
                    )}
                    <span className="flex-1 truncate">{e.kind === "space" ? displayPath(e.path) : e.name}</span>
                    <ChevronRight className="size-4 text-gray-400" />
                  </button>
                ))
              )}
            </div>
            {dir !== "/" &&
              !busy &&
              (newFolder === null ? (
                <button className="mt-2 flex items-center gap-1.5 self-start text-sm text-blue-700 hover:underline" onClick={() => setNewFolder("")}>
                  <FolderPlus className="size-4" /> New folder
                </button>
              ) : (
                <div className="mt-2 flex gap-2">
                  <input
                    className="input flex-1"
                    autoFocus
                    placeholder="Folder name"
                    value={newFolder}
                    onChange={(e) => setNewFolder(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && void createFolder()}
                  />
                  <button className="btn text-sm" onClick={() => void createFolder()}>
                    Create
                  </button>
                </div>
              ))}

            {phase.kind === "uploading" && (
              <div className="mt-4">
                <div className="h-1.5 overflow-hidden rounded bg-gray-200">
                  <div
                    className="h-full bg-blue-600 transition-[width]"
                    style={{ width: `${phase.total ? Math.round((phase.sent / phase.total) * 100) : 0}%` }}
                  />
                </div>
                <p className="mt-1 text-xs text-gray-500">
                  {phase.total ? `${bytes(phase.sent)} of ${bytes(phase.total)}` : "Preparing…"}
                </p>
              </div>
            )}
            {err && <p className="mt-3 text-sm text-red-700">{err}</p>}

            <div className="mt-4 flex items-center justify-end gap-2">
              {dir === "/" && <span className="mr-auto text-xs text-gray-500">Choose Shared or My files</span>}
              <button className="btn text-sm" onClick={cancel} disabled={busy}>
                Cancel
              </button>
              <button className="btn btn-primary text-sm" onClick={() => void save()} disabled={!canSave}>
                {busy ? "Saving…" : phase.kind === "nameTaken" ? "Save with this name" : `Save to ${displayPath(dir).split(" › ").pop()}`}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
