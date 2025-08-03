import { createI18n } from 'vue-i18n'

// Create a vue-i18n instance with empty messages.
// Locales and messages will be loaded dynamically at runtime.
export const i18n = createI18n({
  legacy: false, // Use Composition API mode
  locale: 'zh-CN', // Default locale (will be overridden when initialized)
  messages: {}, // No preloaded messages
})
