import type { Locale as AntdLocale } from 'antdv-next/dist/locale/index'

import antdEnUS from 'antdv-next/locale/en_US'
import antdPtBR from 'antdv-next/locale/pt_BR'
import antdViVN from 'antdv-next/locale/vi_VN'
import antdZhCN from 'antdv-next/locale/zh_CN'
import antdZhTW from 'antdv-next/locale/zh_TW'
import { createI18n } from 'vue-i18n'

import type { Language } from '@/stores/general'

import { LANGUAGE } from '@/constants'

import enUS from './en-US.json'
import ptBR from './pt-BR.json'
import viVN from './vi-VN.json'
import zhCN from './zh-CN.json'
import zhTW from './zh-TW.json'

export const i18n = createI18n({
  legacy: false,
  locale: LANGUAGE.EN_US,
  fallbackLocale: LANGUAGE.EN_US,
  messages: {
    [LANGUAGE.ZH_CN]: zhCN,
    [LANGUAGE.ZH_TW]: zhTW,
    [LANGUAGE.EN_US]: enUS,
    [LANGUAGE.VI_VN]: viVN,
    [LANGUAGE.PT_BR]: ptBR,
  },
})

export function getAntdLocale(language: Language = LANGUAGE.EN_US) {
  const antdLanguage: Record<Language, AntdLocale> = {
    [LANGUAGE.ZH_CN]: antdZhCN,
    [LANGUAGE.ZH_TW]: antdZhTW,
    [LANGUAGE.EN_US]: antdEnUS,
    [LANGUAGE.VI_VN]: antdViVN,
    [LANGUAGE.PT_BR]: antdPtBR,
  }

  return antdLanguage[language]
}
