import { defineNuxtConfig } from 'nuxt/config';

export default defineNuxtConfig({
  compatibilityDate: '2026-11-16',
  ssr: false,
  telemetry: false,
  devtools: { enabled: false },
  devServer: { port: 1420 },
  app: {
    rootId: 'root',
    head: {
      title: process.env.NODE_ENV === 'development' ? 'DataGoblin (development)' : 'DataGoblin',
      htmlAttrs: { lang: 'en' },
      link: [{ rel: 'icon', type: 'image/svg+xml', href: '/goblin.svg' }],
      meta: [{ name: 'theme-color', content: '#101513' }],
    },
  },
  css: [
    '@fontsource-variable/inter', '@fontsource-variable/inter/wght-italic.css',
    '@fontsource-variable/jetbrains-mono', '@fontsource-variable/jetbrains-mono/wght-italic.css',
    '~/../src/themes.css', '~/../src/style.css',
  ],
  vite: { clearScreen: false, envPrefix: ['VITE_', 'TAURI_'], server: { strictPort: true } },
  ignore: ['**/src-tauri/**', '**/tmp/**', '**/performance-results/**'],
});
