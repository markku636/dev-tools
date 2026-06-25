import type { LucideIcon, LucideProps } from "lucide-react";

/**
 * 薄包裝：統一 lucide 圖示的尺寸與線寬，並把整個圖示庫收斂到單一進入點，
 * 日後若要換圖示庫只需改這裡。圖示以 currentColor 上色，故 className 給
 * text-* 即可隨主題翻轉（例：text-mysql、text-fg/50、text-accent）。
 *
 * 尺寸慣例：工具列 20、介面 / 表格 16、密集樹狀 14。勿混用 stroke 粗細。
 * 透傳所有標準 lucide / SVG props（title、onClick、style…），呼叫端更自由。
 */
export interface IconProps extends Omit<LucideProps, "ref"> {
  icon: LucideIcon;
  /** 提示文字：映射為 aria-label（SVG title 屬性無法當原生 tooltip）。 */
  title?: string;
}

export default function Icon({ icon: Glyph, size = 16, strokeWidth = 1.75, title, ...rest }: IconProps) {
  return (
    <Glyph
      size={size}
      strokeWidth={strokeWidth}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      focusable={false}
      {...rest}
    />
  );
}
