/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  // 亮色主題以 <html class="light"> 覆寫 CSS 變數切換；深色為預設（:root）。
  darkMode: ["selector", '[class~="light"] &'],
  theme: {
    extend: {
      colors: {
        // ---- 語意化表面 / 文字（以 CSS 變數驅動，明暗主題自動翻轉）----
        // 用 rgb(var(--x) / <alpha-value>) 形式，讓 text-fg/60、border-fg/10 等透明度語法照常運作。
        app: "rgb(var(--c-app) / <alpha-value>)", // 應用底色（最底層）
        panel: "rgb(var(--c-panel) / <alpha-value>)", // 側欄 / 面板 / 狀態列
        bar: "rgb(var(--c-bar) / <alpha-value>)", // 上方工具列
        elevated: "rgb(var(--c-elevated) / <alpha-value>)", // 對話框 / 選單 / 卡片
        inset: "rgb(var(--c-inset) / <alpha-value>)", // 凹陷面：輸入框 / 區塊
        well: "rgb(var(--c-well) / <alpha-value>)", // 更深的凹陷：程式碼 / 結果區
        fg: "rgb(var(--c-fg) / <alpha-value>)", // 前景文字（深色=白、亮色=近黑）
        accent: "rgb(var(--c-accent) / <alpha-value>)", // 主強調色（隨主題加深）
        // 語意意圖色（深 / 亮各定義於 styles.css，透明度語法照常）
        success: "rgb(var(--c-success) / <alpha-value>)",
        warning: "rgb(var(--c-warning) / <alpha-value>)",
        danger: "rgb(var(--c-danger) / <alpha-value>)",
        info: "rgb(var(--c-info) / <alpha-value>)",
        // 連線類型色標（兩種主題通用）
        mysql: "#3b82f6",
        postgres: "#6366f1",
        mongo: "#22c55e",
        redis: "#ef4444",
      },
      // 圓角刻度：控制項=sm(5)、容器=md(8)、modal=lg(12)、徽章=xs(3)。
      // DEFAULT=sm 使裸 rounded 由 4→5px（全域刻意微調）。
      borderRadius: {
        xs: "var(--r-xs)",
        sm: "var(--r-sm)",
        DEFAULT: "var(--r-sm)",
        md: "var(--r-md)",
        lg: "var(--r-lg)",
        full: "var(--r-full)",
      },
      // 景深刻度（深 / 亮各定義於 styles.css）：shadow-e1…e4。
      boxShadow: {
        e1: "var(--e-1)",
        e2: "var(--e-2)",
        e3: "var(--e-3)",
        e4: "var(--e-4)",
      },
    },
  },
  plugins: [],
};
