import { Loader2 } from "lucide-react";
import Icon from "./Icon";

export interface SpinnerProps {
  size?: number;
  className?: string;
}

/** 載入轉圈：統一取代各處手刻的 border-t-transparent animate-spin。 */
export default function Spinner({ size = 14, className = "text-fg/50" }: SpinnerProps) {
  return <Icon icon={Loader2} size={size} className={`animate-spin ${className}`} />;
}
