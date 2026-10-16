import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  clearScreen: false,
  server: { port: 5174, strictPort: true, fs: { allow: [".", "../../../assets"] } },
  build: { target: "es2022" },
  worker: { format: "es" },
});
