/** @type {import('tailwindcss').Config} */
module.exports = {
  // Scan all Rust source files for Tailwind class names
  content: ["./src/**/*.rs", "./index.html"],
  theme: {
    extend: {
      colors: {
        // Wind Swap brand palette
        wind: {
          50:  "#f0f9ff",
          100: "#e0f2fe",
          400: "#38bdf8",
          500: "#0ea5e9",
          600: "#0284c7",
          900: "#0c4a6e",
        },
        surface: {
          DEFAULT: "#111827", // gray-900
          raised:  "#1f2937", // gray-800
          border:  "#374151", // gray-700
        },
      },
      borderRadius: {
        "2xl": "1rem",
        "3xl": "1.5rem",
      },
      animation: {
        "spin-slow": "spin 3s linear infinite",
        "pulse-fast": "pulse 1s ease-in-out infinite",
        "fade-in": "fadeIn 0.2s ease-out",
      },
      keyframes: {
        fadeIn: {
          "0%":   { opacity: "0", transform: "translateY(4px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
      },
    },
  },
  plugins: [],
};
