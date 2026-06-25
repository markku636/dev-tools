import type { ReactNode } from "react";

export interface FieldProps {
  label?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  required?: boolean;
  htmlFor?: string;
  className?: string;
  children: ReactNode;
}

/** 表單列：標籤 + 控制項 + hint / error，統一垂直節奏。 */
export default function Field({ label, hint, error, required, htmlFor, className = "", children }: FieldProps) {
  return (
    <div className={`grid gap-1.5 ${className}`}>
      {label !== undefined && (
        <label htmlFor={htmlFor} className="text-xs text-fg/60 select-none">
          {label}
          {required && <span className="text-danger ml-0.5">*</span>}
        </label>
      )}
      {children}
      {error ? (
        <div className="text-[11px] text-danger">{error}</div>
      ) : hint ? (
        <div className="text-xs text-fg/40">{hint}</div>
      ) : null}
    </div>
  );
}

/** 多欄表單容器。 */
export function FormGrid({ className = "", children }: { className?: string; children: ReactNode }) {
  return <div className={`grid gap-4 ${className}`}>{children}</div>;
}
