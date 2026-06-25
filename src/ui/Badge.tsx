import type { ReactNode } from "react";

export type BadgeTone = "neutral" | "info" | "success" | "warning" | "danger" | "accent";

const tones: Record<BadgeTone, string> = {
  neutral: "bg-fg/8 text-fg/55",
  info: "bg-info/15 text-info",
  success: "bg-success/15 text-success",
  warning: "bg-warning/15 text-warning",
  danger: "bg-danger/15 text-danger",
  accent: "bg-accent/15 text-accent",
};

export interface BadgeProps {
  tone?: BadgeTone;
  dot?: boolean;
  className?: string;
  children: ReactNode;
}

/** 小徽章：列數 / 狀態 / PK / NULL 旗標等。 */
export default function Badge({ tone = "neutral", dot = false, className = "", children }: BadgeProps) {
  return (
    <span
      className={`inline-flex items-center gap-1 h-[18px] px-1.5 rounded-xs text-[11px] font-medium tabular-nums ${tones[tone]} ${className}`}
    >
      {dot && <span className="w-1.5 h-1.5 rounded-full bg-current opacity-80" />}
      {children}
    </span>
  );
}
