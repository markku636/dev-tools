import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import Icon from "./Icon";

export interface EmptyStateProps {
  icon?: LucideIcon;
  title: ReactNode;
  hint?: ReactNode;
  /** 選用行動鈕（通常是 <Button> 或一組按鈕）。 */
  action?: ReactNode;
  /** 緊湊版：用於對話框內 / 結果面板等較小區域。 */
  compact?: boolean;
  className?: string;
}

/**
 * 統一空狀態：置中的柔色圖示徽章 + 標題 + 提示 + 選用行動鈕。
 * 收斂散落各處的 text-fg/30..40 單行空狀態（主畫面 / 結果面板 / 對話框 / 表格），
 * 讓「沒有資料 / 尚未連線 / 查無結果」都有一致、可帶 CTA 的呈現。
 */
export default function EmptyState({
  icon,
  title,
  hint,
  action,
  compact = false,
  className = "",
}: EmptyStateProps) {
  return (
    <div
      className={
        `flex flex-col items-center justify-center text-center select-none ` +
        (compact ? "gap-2 p-6 " : "gap-3 p-10 ") +
        className
      }
    >
      {icon && (
        <div
          className={
            `grid place-items-center rounded-full bg-fg/5 text-fg/30 ` +
            (compact ? "w-10 h-10" : "w-14 h-14")
          }
        >
          <Icon icon={icon} size={compact ? 20 : 26} strokeWidth={1.5} />
        </div>
      )}
      <div className={`text-fg/55 ${compact ? "text-xs" : "text-sm"}`}>{title}</div>
      {hint && <div className="text-xs text-fg/35 max-w-[42ch] leading-relaxed">{hint}</div>}
      {action && <div className="mt-1 flex items-center gap-2">{action}</div>}
    </div>
  );
}
