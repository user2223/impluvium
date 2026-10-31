// https://nuxt.com/docs/api/configuration/nuxt-config
import { fileURLToPath } from "node:url";

export default defineNuxtConfig({
  compatibilityDate: "2024-04-03",

  postcss: {
    plugins: {
      tailwindcss: {},
      autoprefixer: {},
    },
  },

  css: ["~/assets/main.scss"],

  ssr: false,
  devtools: false,

  extends: [["../../libraries/base"]],

  // The client loads the UI from the "main" folder of its frontend directory.
  app: {
    baseURL: "/main/",
  },

  // `nuxt generate` writes the static UI straight to desktop/.output/main; Tauri serves desktop/.output
  // (frontendDist in src-tauri/tauri.conf.json).
  nitro: {
    output: {
      publicDir: fileURLToPath(new URL("../.output/main", import.meta.url)),
    },
  },
});
