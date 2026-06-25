import type { ButtonHTMLAttributes } from "react";
import type { LucideIcon } from "lucide-react";
import Icon from "./Icon";

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "title"> {
  icon: LucideIcon;
  /** 無障礙標籤（同時作 title tooltip）。 */
  label: string;
  /** 圖示尺寸（px）。 */
  iconSize?: number;
  /** 按鈕方框尺寸 class，預設 w-7 h-7。 */
  box?: string;
  active?: boolean;
}

/** 純圖示按鈕：取代散落各處的 ✕ 關閉鈕與工具磚。 */
export default function IconButton({
  icon,
  label,
  iconSize = 16,
  box = "w-7 h-7",
  active = false,
  className = "",
  type = "button",
  ...rest
}: IconButtonProps) {
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      className={
        `${box} grid place-items-center rounded shrink-0 transition-colors ` +
        `disabled:opacity-40 disabled:pointer-events-none focus-visible:outline-2 focus-visible:outline-accent/60 ` +
        (active
          ? "bg-accent/12 text-accent "
          : "text-fg/55 hover:text-fg hover:bg-fg/10 active:bg-fg/[0.14] ") +
        className
      }
      {...rest}
    >
      <Icon icon={icon} size={iconSize} />
    </button>
  );
}
