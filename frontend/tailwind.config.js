/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        paper: "rgb(var(--paper) / <alpha-value>)",
        surface: "rgb(var(--surface) / <alpha-value>)",
        raised: "rgb(var(--raised) / <alpha-value>)",
        ink: "rgb(var(--ink) / <alpha-value>)",
        muted: "rgb(var(--muted) / <alpha-value>)",
        faint: "rgb(var(--faint) / <alpha-value>)",
        line: "rgb(var(--line) / <alpha-value>)",
        accent: "rgb(var(--accent) / <alpha-value>)",
        "accent-strong": "rgb(var(--accent-strong) / <alpha-value>)",
        "accent-soft": "rgb(var(--accent-soft) / <alpha-value>)",
        danger: "rgb(var(--danger) / <alpha-value>)",
        "danger-soft": "rgb(var(--danger-soft) / <alpha-value>)",
        success: "rgb(var(--success) / <alpha-value>)",
        "success-soft": "rgb(var(--success-soft) / <alpha-value>)",
        warning: "rgb(var(--warning) / <alpha-value>)",
        "warning-soft": "rgb(var(--warning-soft) / <alpha-value>)",
        info: "rgb(var(--info) / <alpha-value>)",
        "info-soft": "rgb(var(--info-soft) / <alpha-value>)"
      },
      fontFamily: {
        display: ["Iowan Old Style", "Palatino Linotype", "Noto Serif SC", "Songti SC", "STSong", "serif"],
        sans: ["Avenir Next", "Noto Sans SC", "PingFang SC", "Microsoft YaHei", "sans-serif"],
        mono: ["JetBrains Mono", "SFMono-Regular", "Menlo", "monospace"]
      },
      boxShadow: {
        card: "0 1px 2px rgb(var(--shadow) / 0.04), 0 12px 34px -20px rgb(var(--shadow) / 0.28)",
        float: "0 18px 60px -18px rgb(var(--shadow) / 0.42)",
        ring: "0 0 0 1px rgb(var(--line) / 0.9)"
      },
      borderRadius: {
        xl: "0.9rem",
        "2xl": "1.25rem"
      },
      transitionDuration: {
        DEFAULT: "160ms",
        200: "200ms"
      },
      opacity: {
        8: "0.08",
        15: "0.15",
        35: "0.35",
        45: "0.45",
        55: "0.55",
        65: "0.65",
        85: "0.85"
      },
      keyframes: {
        "fade-up": {
          "0%": { opacity: "0", transform: "translateY(8px)" },
          "100%": { opacity: "1", transform: "translateY(0)" }
        },
        "fade-in": {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" }
        },
        "slide-in": {
          "0%": { opacity: "0", transform: "translateX(10px)" },
          "100%": { opacity: "1", transform: "translateX(0)" }
        },
        shimmer: {
          "0%": { backgroundPosition: "200% 0" },
          "100%": { backgroundPosition: "-200% 0" }
        },
        "pulse-soft": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.45" }
        },
        indeterminate: {
          "0%": { transform: "translateX(-120%)" },
          "100%": { transform: "translateX(420%)" }
        }
      },
      animation: {
        "fade-up": "fade-up 180ms ease-out both",
        "fade-in": "fade-in 160ms ease-out both",
        "slide-in": "slide-in 180ms ease-out both",
        shimmer: "shimmer 1.6s linear infinite",
        "pulse-soft": "pulse-soft 1.6s ease-in-out infinite",
        indeterminate: "indeterminate 1.1s ease-in-out infinite"
      }
    }
  },
  plugins: []
};
