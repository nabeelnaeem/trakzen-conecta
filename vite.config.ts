import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as { version: string };

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],

  // Notification sounds are inlined into the bundle (see src/lib/notify.ts);
  // a few hundred KB more in a desktop app is fine.
  build: { chunkSizeWarningLimit: 1500 },

  // Shown under Settings → About.
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    // Full timestamp so builds from the same day are distinguishable.
    __BUILD_DATE__: JSON.stringify(new Date().toISOString().replace("T", " ").slice(0, 16) + " UTC"),
    __BUILD_ID__: JSON.stringify(Math.floor(Date.now() / 1000).toString(36)),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available.
  //    1470 rather than Tauri's default 1420 so this can run alongside
  //    other Tauri projects in development.
  server: {
    port: 1470,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1471,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
