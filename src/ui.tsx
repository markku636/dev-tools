import { create } from "zustand";
import { open, save } from "@tauri-apps/plugin-dialog";

// ---- Toast 通知 + 確認對話框 共用狀態 ----

export interface Toast {
  id: number;
  kind: "success" | "error" | "info";
  text: string;
}

interface ConfirmReq {
  message: string;
  title?: string;
  danger?: boolean;
  confirmText?: string;
  resolve: (ok: boolean) => void;
}

interface UiStore {
  toasts: Toast[];
  confirmReq: ConfirmReq | null;
  pushToast: (kind: Toast["kind"], text: string) => void;
  dismissToast: (id: number) => void;
  requestConfirm: (req: ConfirmReq) => void;
  resolveConfirm: (ok: boolean) => void;
}

let toastSeq = 1;

export const useUi = create<UiStore>((set, get) => ({
  toasts: [],
  confirmReq: null,
  pushToast: (kind, text) => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, text }] }));
    const ttl = kind === "error" ? 6000 : 3200;
    setTimeout(() => get().dismissToast(id), ttl);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  requestConfirm: (req) => set({ confirmReq: req }),
  resolveConfirm: (ok) => {
    const req = get().confirmReq;
    set({ confirmReq: null });
    req?.resolve(ok);
  },
}));

export const toast = {
  success: (t: string) => useUi.getState().pushToast("success", t),
  error: (t: string) => useUi.getState().pushToast("error", t),
  info: (t: string) => useUi.getState().pushToast("info", t),
};

/** 以 Promise 取代瀏覽器 confirm()，配合 <UiHost /> 的樣式化對話框。 */
export function uiConfirm(
  message: string,
  opts?: { title?: string; danger?: boolean; confirmText?: string }
): Promise<boolean> {
  return new Promise((resolve) => {
    useUi.getState().requestConfirm({ message, resolve, ...opts });
  });
}

// ---- 原生檔案選擇器（Tauri dialog plugin）----

type Filter = { name: string; extensions: string[] };

export async function pickOpenFile(filters?: Filter[]): Promise<string | null> {
  const res = await open({ multiple: false, directory: false, filters });
  return typeof res === "string" ? res : null;
}

export async function pickDirectory(): Promise<string | null> {
  const res = await open({ multiple: false, directory: true });
  return typeof res === "string" ? res : null;
}

export async function pickSaveFile(defaultPath?: string, filters?: Filter[]): Promise<string | null> {
  const res = await save({ defaultPath, filters });
  return res ?? null;
}

// ---- 掛在 App 根的通知 / 確認渲染層 ----

export function UiHost() {
  const { toasts, dismissToast, confirmReq, resolveConfirm } = useUi();

  const kindStyle = (k: Toast["kind"]) =>
    k === "success"
      ? "border-green-500/40 bg-green-500/15 text-green-200"
      : k === "error"
      ? "border-red-500/40 bg-red-500/15 text-red-200"
      : "border-white/15 bg-white/10 text-white/80";

  return (
    <>
      <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 w-80 max-w-[90vw]">
        {toasts.map((t) => (
          <div
            key={t.id}
            onClick={() => dismissToast(t.id)}
            className={`px-3 py-2 rounded-md shadow-lg text-sm border cursor-pointer break-words ${kindStyle(t.kind)}`}
          >
            {t.text}
          </div>
        ))}
      </div>

      {confirmReq && (
        <div
          className="fixed inset-0 bg-black/50 flex items-center justify-center z-[110]"
          onClick={() => resolveConfirm(false)}
        >
          <div
            className="bg-[#1a212b] w-[380px] max-w-[92vw] rounded-lg border border-white/10 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="px-5 py-3 border-b border-white/10 font-medium text-sm">
              {confirmReq.title ?? "確認"}
            </div>
            <div className="p-5 text-sm text-white/80 whitespace-pre-wrap break-words">
              {confirmReq.message}
            </div>
            <div className="px-5 py-3 border-t border-white/10 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => resolveConfirm(false)}
                className="px-3 py-1.5 text-sm rounded border border-white/15 hover:bg-white/5"
              >
                取消
              </button>
              <button
                type="button"
                onClick={() => resolveConfirm(true)}
                className={`px-3 py-1.5 text-sm rounded ${
                  confirmReq.danger ? "bg-red-600 hover:bg-red-500" : "bg-blue-600 hover:bg-blue-500"
                }`}
              >
                {confirmReq.confirmText ?? "確定"}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
