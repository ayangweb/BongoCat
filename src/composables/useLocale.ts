// useLocale.ts
import { resolveResource } from '@tauri-apps/api/path'
import { readDir, readTextFile } from '@tauri-apps/plugin-fs'
import enUS from 'ant-design-vue/es/locale/en_US'
import viVN from 'ant-design-vue/es/locale/vi_VN'
import zhCN from 'ant-design-vue/es/locale/zh_CN'
import { ref, watch } from 'vue'

import { i18n } from '@/i18n'
import { useGeneralStore } from '@/stores/general'

// Current Ant Design Vue locale (reactive)
export const antLocale = ref(zhCN)

// Mapping of supported Ant Design Vue locales
const antLocales: Record<string, any> = {
  'zh-CN': zhCN,
  'en-US': enUS,
  'vi-VN': viVN,
}

// List of all available locale codes (from JSON files)
let availableLocales: string[] = []

// Load all available locale codes from the assets/locales directory
export async function loadAvailableLocales() {
  const localesPath = await resolveResource('assets/locales')
  const files = await readDir(localesPath)
  availableLocales = files
    .filter(f => f.name.endsWith('.json'))
    .map(f => f.name.replace('.json', ''))
  return availableLocales
}

// Flag to indicate locale initialization is done
const isLocaleReady = ref(false)
// Holds meta info (code & display name) of each locale
const locales = ref<{ code: string, name: string }[]>([])

export function useLocale() {
  const generalStore = useGeneralStore()

  // Initialize the locale at app startup
  async function initLocale() {
    const codes = await loadAvailableLocales()

    // Only load meta.name from each locale file (don't set i18n yet)
    locales.value = await Promise.all(
      codes.map(async (code) => {
        const messages = await loadLocaleMessages(code)
        return {
          code,
          name: messages.meta?.name || code,
        }
      }),
    )

    // If no saved locale, set default
    if (!generalStore.locale) {
      generalStore.locale = 'zh-CN'
    }

    // Set the initial locale once all locales are loaded
    await setLocale(generalStore.locale)
    isLocaleReady.value = true
  }

  // Watch for locale changes and update dynamically
  watch(
    () => generalStore.locale,
    async (val) => {
      if (val && isLocaleReady.value) {
        await setLocale(val)
      }
    },
  )

  return { generalStore, locales, isLocaleReady, initLocale }
}

// Load locale message JSON by code
export async function loadLocaleMessages(locale: string) {
  const filePath = await resolveResource(`assets/locales/${locale}.json`)
  const jsonContent = await readTextFile(filePath)
  return JSON.parse(jsonContent)
}

// Cache to avoid reloading already loaded locales
const loadedLocales = new Set<string>()

// Set the active locale for vue-i18n and Ant Design Vue
export async function setLocale(locale: string) {
  if (!loadedLocales.has(locale)) {
    const messages = await loadLocaleMessages(locale)
    i18n.global.setLocaleMessage(locale, messages)
    loadedLocales.add(locale)
  }
  // Update vue-i18n locale
  i18n.global.locale.value = locale

  // Update Ant Design Vue locale (fallback to enUS if not found)
  antLocale.value = antLocales[locale] || enUS
}
