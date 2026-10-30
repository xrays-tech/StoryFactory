import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 开发期把 /api 代理到本地后端；SSE 需要关闭缓冲并保持长连接。
const proxyTarget = process.env.VITE_API_PROXY_TARGET ?? "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: proxyTarget,
        changeOrigin: true,
        // 后端 HostGuard 校验 Host，OriginGuard 校验写请求 Origin。
        // 代理层统一改写为 127.0.0.1:8080，本地开发无需开启 dev_cors 也能用。
        configure: (proxy) => {
          proxy.on("proxyReq", (proxyReq) => {
            proxyReq.setHeader("origin", proxyTarget);
          });
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});
