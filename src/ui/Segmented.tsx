import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import Icon from "./Icon";

export interface SegmentedOption<T extends string> {
  value: T;
  label: ReactNode;
  icon?: LucideIcon;
  title?: string;
  disabled?: boolean;
}

export interface SegmentedProps<T extends string> {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  size?: "sm" | "md";
  /** 撐滿容器寬度，各段平均分配。 */
  full?: boolean;
  ariaLabel?: string;
  className?: string;
}

/**
 * 分段切換（role=radiogroup）：取代各對話框手刻的「兩三顆並排切換鈕」。
 * 作用中段以實心 accent 標示（白字，跨主題穩定）；其餘為低調的 ghost。
 * 保留原生 radio 語意，鍵盤 Tab 可進、方向鍵由瀏覽器處理。
 */
export default function Segmented<T extends string>({
  options,
  value,
  onChange,
  size = "sm",
  full = false,
  ariaLabel,
  className = "",
}: SegmentedProps<T>) {
  const h = size === "sm" ? "h-7 text-[13px]" : "h-8 text-sm";
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className={
        `inline-flex items-center gap-0.5 p-0.5 rounded-md bg-inset border border-fg/10 ` +
        (full ? "w-full " : "") +
        className
      }
    >
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={o.disabled}
            title={o.title}
            onClick={() => onChange(o.value)}
            className={
              `inline-flex items-center justify-center gap-1.5 px-3 rounded ${h} ` +
              (full ? "flex-1 " : "") +
              `font-medium transition-colors disabled:opacity-40 disabled:pointer-events-none ` +
              `focus-visible:outline-2 focus-visible:outline-accent/60 ` +
              (active
                ? "bg-accent text-white shadow-e1"
                : "text-fg/60 hover:text-fg hover:bg-fg/10")
            }
          >
            {o.icon && <Icon icon={o.icon} size={14} />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
