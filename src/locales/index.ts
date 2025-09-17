import type { Language } from '@/stores/general'
import type { Locale as AntdLocale } from 'ant-design-vue/es/locale'

import antdEnUS from 'ant-design-vue/locale/en_US'
import antdViVN from 'ant-design-vue/locale/vi_VN'
import antdZhCN from 'ant-design-vue/locale/zh_CN'
import { createI18n } from 'vue-i18n'

import enUS from './en-US.json'
import viVN from './vi-VN.json'
import zhCN from './zh-CN.json'

import { LANGUAGE } from '@/constants'

export const i18n = createI18n({
  legacy: false,
  locale: LANGUAGE.EN_US,
  fallbackLocale: LANGUAGE.EN_US,
  messages: {
    [LANGUAGE.ZH_CN]: zhCN,
    [LANGUAGE.EN_US]: enUS,
    [LANGUAGE.VI_VN]: viVN,
  },
})

export function getAntdLocale(language: Language = LANGUAGE.EN_US) {
  const antdLanguage: Record<Language, AntdLocale> = {
    [LANGUAGE.ZH_CN]: antdZhCN,
    [LANGUAGE.EN_US]: antdEnUS,
    [LANGUAGE.VI_VN]: antdViVN,
  }

  return antdLanguage[language]
}
