import { createI18n } from 'vue-i18n'

import en from '@/locales/en'
import zh from '@/locales/zh'
import { useAppStore } from '@/stores/app'

const i18n = createI18n({
  legacy: false,
  locale: 'zh',
  fallbackLocale: 'en',
  messages: {
    en,
    zh,
  },
})

export function useI18n() {
  const appStore = useAppStore()

  function setLocale(locale: 'zh' | 'en') {
    i18n.global.locale.value = locale
    appStore.locale = locale
  }

  return {
    setLocale,
  }
}

export default i18n
