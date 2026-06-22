/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // 連線類型色標
        mysql: "#3b82f6",
        postgres: "#6366f1",
        mongo: "#22c55e",
        redis: "#ef4444",
      },
    },
  },
  plugins: [],
};
