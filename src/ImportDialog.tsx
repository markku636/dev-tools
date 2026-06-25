import { useState } from "react";
import { api, ImportResult } from "./api";
import { pickOpenFile, toast } from "./ui";
import { Modal, Button, Segmented, Input } from "./ui/index";
import { Download } from "lucide-react";

// CSV 匯入對話框（致敬 Navicat / DBeaver 匯入精靈）。逐列以 insert_row 寫入目標表。
export default function ImportDialog({ connId, database, table, onDone, onClose }: {
  connId: string;
  database: string;
  table: string;
  onDone?: () => void;
  onClose: () => void;
}) {
  const [delimiter, setDelimiter] = useState(",");
  const [hasHeader, setHasHeader] = useState(true);
  const [emptyAsNull, setEmptyAsNull] = useState(true);
  const [stopOnError, setStopOnError] = useState(false);
  const [columns, setColumns] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ImportResult | null>(null);

  const run = async () => {
    if (busy) return; // 防重入：開檔對話框期間避免重複觸發
    const cols = hasHeader ? null : columns.split(",").map((c) => c.trim()).filter(Boolean);
    if (!hasHeader && (!cols || cols.length === 0)) {
      toast.error("無表頭時請先填欄名（逗號分隔）");
      return;
    }
    const path = await pickOpenFile([{ name: "CSV / TSV", extensions: ["csv", "tsv", "txt"] }]);
    if (!path) return;
    setBusy(true);
    setResult(null);
    try {
      const res = await api.importCsv(connId, database, table, path, {
        delimiter,
        has_header: hasHeader,
        empty_as_null: emptyAsNull,
        columns: cols,
        stop_on_error: stopOnError,
      });
      setResult(res);
      if (res.failed === 0) {
        toast.success(`已匯入 ${res.imported} 列`);
      } else {
        toast.error(`匯入 ${res.imported} 列、失敗 ${res.failed} 列`);
      }
      onDone?.(); // 重新整理資料格以顯示已匯入的列
    } catch (e: any) {
      toast.error(e?.message ?? "匯入失敗");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      onClose={onClose}
      title={<>匯入 CSV · <span className="mono text-fg/60">{table}</span></>}
      icon={Download}
      size="sm"
      zClass="z-50"
      className="!w-[460px]"
      bodyClassName="p-5 space-y-3 overflow-auto"
      footer={<>
        <Button variant="secondary" onClick={onClose}>{result ? "關閉" : "取消"}</Button>
        <Button variant="primary" loading={busy} onClick={run} disabled={busy}>選擇檔案並匯入</Button>
      </>}
    >
      <div className="flex items-center gap-3">
            <span className="text-xs text-fg/50">分隔字元</span>
            <Segmented
              ariaLabel="分隔字元"
              value={delimiter}
              onChange={setDelimiter}
              options={[
                { value: ",", label: "逗號 ," },
                { value: "\t", label: "Tab" },
                { value: ";", label: "分號 ;" },
              ]}
            />
          </div>

          <label className="flex items-center gap-2 text-sm cursor-pointer select-none">
            <input type="checkbox" checked={hasHeader} onChange={(e) => setHasHeader(e.target.checked)} />
            第一列為欄名
          </label>
          {!hasHeader && (
            <label className="block">
              <span className="text-xs text-fg/50 mb-1 block">欄名（逗號分隔，依 CSV 欄序對應）</span>
              <Input inputSize="md" value={columns} onChange={(e) => setColumns(e.target.value)}
                placeholder="id, name, qty" />
            </label>
          )}
          <label className="flex items-center gap-2 text-sm cursor-pointer select-none">
            <input type="checkbox" checked={emptyAsNull} onChange={(e) => setEmptyAsNull(e.target.checked)} />
            空欄位視為 NULL（建議開：避免空字串塞進數值 / 日期欄而失敗）
          </label>
          <label className="flex items-center gap-2 text-sm cursor-pointer select-none">
            <input type="checkbox" checked={stopOnError} onChange={(e) => setStopOnError(e.target.checked)} />
            遇錯即停（取消＝盡量匯入，回報失敗列與錯誤）
          </label>

          {result && (
            <div className="mt-1 text-sm rounded border border-fg/10 bg-inset p-3 space-y-1">
              <div>
                匯入 <span className="text-emerald-400">{result.imported}</span> 列
                {result.failed > 0 && <> · 失敗 <span className="text-red-400">{result.failed}</span> 列</>}
              </div>
              {result.errors.length > 0 && (
                <ul className="text-xs text-red-300/80 mono max-h-32 overflow-auto list-disc pl-4">
                  {result.errors.map((er, i) => <li key={i}>{er}</li>)}
                </ul>
              )}
            </div>
          )}
    </Modal>
  );
}

