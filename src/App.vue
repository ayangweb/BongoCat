<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { error } from '@tauri-apps/plugin-log'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useEventListener } from '@vueuse/core'
import { ConfigProvider, theme } from 'ant-design-vue'
import { isString } from 'es-toolkit'
import isURL from 'is-url'
import { onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterView } from 'vue-router'

import type { RuntimeLabelPayload } from '@/stores/label'

import { useTauriListen } from './composables/useTauriListen'
import { useThemeVars } from './composables/useThemeVars'
import { useWindowState } from './composables/useWindowState'
import { INVOKE_KEY, LANGUAGE, LISTEN_KEY, WINDOW_LABEL } from './constants'
import { getAntdLocale } from './locales/index.ts'
import { hideWindow, showWindow } from './plugins/window'
import { useAppStore } from './stores/app'
import { useCatStore } from './stores/cat'
import { useGeneralStore } from './stores/general'
import { useLabelStore } from './stores/label'
import { useModelStore } from './stores/model'
import { useShortcutStore } from './stores/shortcut.ts'

interface ClaudeEventPayload {
  state: 'idle' | 'thinking' | 'coding' | 'success' | 'error' | 'waiting' | 'dormant'
  source: 'claude' | 'signal'
  sessionId?: string | null
  projectName?: string | null
  detail?: string | null
  rawText?: string | null
  toolName?: string | null
}

function shortenSessionId(sessionId?: string | null) {
  if (!sessionId) {
    return ''
  }

  return sessionId.slice(0, 8)
}

function normalizeSessionValue(value?: string | null) {
  return value
    ?.replace(/\r\n/g, '\n')
    .split('\n')
    .map(line => line.trim())
    .filter(Boolean)
    .join(' ')
    .trim() ?? ''
}

function resolveSessionTitle(payload: ClaudeEventPayload, translate: (key: string, params?: Record<string, unknown>) => string) {
  const detailText = normalizeSessionValue(payload.detail)
  const rawText = normalizeSessionValue(payload.rawText)
  const projectName = normalizeSessionValue(payload.projectName)
  const detailPrefix = detailText.match(/^\[([^\]]+)\]/)?.[1]?.trim()

  if (detailPrefix) {
    return detailPrefix
  }

  if (projectName) {
    const segments = projectName.split('/').filter(Boolean)
    return segments.at(-1) ?? projectName
  }

  if (payload.sessionId) {
    return translate('pages.preference.hook.runtime.sessionFallback', { id: shortenSessionId(payload.sessionId) })
  }

  if (payload.source === 'signal') {
    return ''
  }

  return rawText ? 'Claude' : ''
}

const { generateColorVars } = useThemeVars()
const appStore = useAppStore()
const modelStore = useModelStore()
const catStore = useCatStore()
const labelStore = useLabelStore()
const generalStore = useGeneralStore()
const shortcutStore = useShortcutStore()
const appWindow = getCurrentWebviewWindow()
const { isRestored, restoreState } = useWindowState()
const { darkAlgorithm, defaultAlgorithm } = theme
const { locale, t } = useI18n()

async function syncLabelText(text: string) {
  try {
    await invoke(INVOKE_KEY.SET_LABEL_TEXT, { text })
  } catch (err) {
    await error(`Failed to sync label text: ${String(err)}`)
  }
}

function buildRuntimeLabel(payload: ClaudeEventPayload): RuntimeLabelPayload {
  const signalText = t('pages.preference.hook.runtime.waiting')
  const toolText = payload.toolName
    ? t('pages.preference.hook.runtime.toolWithName', { name: payload.toolName })
    : t('pages.preference.hook.runtime.tool')
  const sessionTitle = resolveSessionTitle(payload, t)

  const text = payload.state === 'waiting'
    ? signalText
    : payload.state === 'dormant'
      ? t('pages.preference.hook.runtime.dormant')
      : payload.state === 'coding'
        ? (labelStore.showDetailedContent ? payload.rawText : null) || toolText
        : payload.state === 'error'
          ? (labelStore.showDetailedContent ? payload.rawText : null) || t('pages.preference.hook.runtime.error')
          : payload.state === 'success'
            ? (labelStore.showDetailedContent ? payload.rawText : null) || t('pages.preference.hook.runtime.success')
            : payload.state === 'thinking'
              ? (labelStore.showDetailedContent ? payload.rawText : null) || t('pages.preference.hook.runtime.thinking')
              : ''

  return {
    text,
    state: payload.state,
    source: payload.source,
    detail: payload.detail,
    rawText: payload.rawText,
    toolName: payload.toolName,
    sessionId: payload.sessionId,
    sessionTitle,
  }
}

onMounted(async () => {
  generateColorVars()

  await appStore.$tauri.start()
  await appStore.init()
  await modelStore.$tauri.start()
  await modelStore.init()
  await catStore.$tauri.start()
  catStore.init()
  await labelStore.$tauri.start()
  await syncLabelText(labelStore.defaultText)
  await generalStore.$tauri.start()
  await generalStore.init()
  await shortcutStore.$tauri.start()
  await restoreState()
})

watch(() => generalStore.appearance.language, (value) => {
  locale.value = value ?? LANGUAGE.EN_US
})

watch(() => labelStore.defaultText, async (value) => {
  if (appWindow.label !== WINDOW_LABEL.PREFERENCE) return

  emit(LISTEN_KEY.LABEL_TEXT_CHANGED, value)
  await syncLabelText(value)
})

watch(() => labelStore.size, (value) => {
  if (appWindow.label !== WINDOW_LABEL.PREFERENCE) return

  emit(LISTEN_KEY.LABEL_SIZE_CHANGED, value)
})

useTauriListen(LISTEN_KEY.SHOW_WINDOW, ({ payload }) => {
  if (appWindow.label !== payload) return

  showWindow()
})

useTauriListen(LISTEN_KEY.HIDE_WINDOW, ({ payload }) => {
  if (appWindow.label !== payload) return

  hideWindow()
})

useTauriListen<string>(LISTEN_KEY.LABEL_TEXT_CHANGED, ({ payload }) => {
  if (appWindow.label !== WINDOW_LABEL.MAIN) return

  labelStore.setDefaultText(payload)
})

useTauriListen<'small' | 'medium' | 'large' | 'xlarge'>(LISTEN_KEY.LABEL_SIZE_CHANGED, ({ payload }) => {
  if (appWindow.label !== WINDOW_LABEL.MAIN) return

  labelStore.size = payload
})

useTauriListen<ClaudeEventPayload>(LISTEN_KEY.CLAUDE_EVENT, ({ payload }) => {
  if (appWindow.label !== WINDOW_LABEL.MAIN) return

  const runtimeLabel = buildRuntimeLabel(payload)

  if (!runtimeLabel.text) {
    labelStore.clearRuntimeLabel(payload.state === 'waiting' ? 'waiting' : 'idle')
    return
  }

  labelStore.setRuntimeLabel(runtimeLabel)
})

useEventListener('unhandledrejection', ({ reason }) => {
  const message = isString(reason) ? reason : JSON.stringify(reason)

  error(message)
})

useEventListener('click', (event) => {
  const link = (event.target as HTMLElement).closest('a')

  if (!link) return

  const { href, target } = link

  if (target === '_blank') return

  event.preventDefault()

  if (!isURL(href)) return

  openUrl(href)
})
</script>

<template>
  <ConfigProvider
    :locale="getAntdLocale(generalStore.appearance.language)"
    :theme="{
      algorithm: generalStore.appearance.isDark ? darkAlgorithm : defaultAlgorithm,
    }"
  >
    <RouterView v-if="isRestored" />
  </ConfigProvider>
</template>
