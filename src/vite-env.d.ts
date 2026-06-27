/// <reference types="vite/client" />

// 由 vite.config.ts 的 define 於建置期注入（值來自 package.json 的 version）。
declare const __APP_VERSION__: string;
