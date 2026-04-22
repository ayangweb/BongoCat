import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { useCatStore } from './cat'

export const LABEL_SIZES = ['small', 'medium', 'large', 'xlarge'] as const
export const LABEL_COLLAPSE_LIMIT = 50
export const SESSION_TITLE_MAX_CHARS = 16

export type LabelSize = typeof LABEL_SIZES[number]
export type RuntimeLabelState = 'idle' | 'thinking' | 'coding' | 'success' | 'error' | 'waiting' | 'dormant'
export type RuntimeLabelSource = 'claude' | 'signal' | 'static'

export interface LabelSizeMetrics {
  fontSize: number
  lineHeight: number
  reservedHeight: number
}

export interface LabelMetrics extends LabelSizeMetrics {
  isVisible: boolean
}

export interface RuntimeLabelPayload {
  text: string
  state: RuntimeLabelState
  source: RuntimeLabelSource
  detail?: string | null
  rawText?: string | null
  toolName?: string | null
  sessionId?: string | null
  sessionTitle?: string | null
}

export const LABEL_SIZE_MAP: Record<LabelSize, LabelSizeMetrics> = {
  small: {
    fontSize: 16,
    lineHeight: 22,
    reservedHeight: 52,
  },
  medium: {
    fontSize: 22,
    lineHeight: 30,
    reservedHeight: 68,
  },
  large: {
    fontSize: 28,
    lineHeight: 36,
    reservedHeight: 80,
  },
  xlarge: {
    fontSize: 34,
    lineHeight: 42,
    reservedHeight: 92,
  },
}

export function truncateLabelText(text: string, maxChars: number): string {
  const normalized = text.trim()

  if (!normalized) {
    return ''
  }

  const chars = [...normalized]

  if (chars.length <= maxChars) {
    return normalized
  }

  return `${chars.slice(0, maxChars).join('')}…`
}

export function normalizeLabelText(text: string): string {
  const normalized = text
    .replace(/\r\n/g, '\n')
    .split('\n')
    .map(line => line.trim().replace(/\s+/g, ' '))
    .filter(Boolean)
    .join('\n')

  return normalized.trim()
}

export function flattenLabelText(text: string): string {
  return normalizeLabelText(text).replace(/\n+/g, ' ').trim()
}

export function buildSessionTitle(text: string): string {
  return truncateLabelText(flattenLabelText(text), SESSION_TITLE_MAX_CHARS)
}

export function getLabelMetrics(text: string, size: LabelSize, scale: number): LabelMetrics {
  const trimmedText = text.trim()

  if (!trimmedText) {
    return {
      fontSize: 0,
      lineHeight: 0,
      reservedHeight: 0,
      isVisible: false,
    }
  }

  const baseMetrics = LABEL_SIZE_MAP[size]
  const factor = scale / 100

  return {
    fontSize: Math.max(1, Math.round(baseMetrics.fontSize * factor)),
    lineHeight: Math.max(1, Math.round(baseMetrics.lineHeight * factor)),
    reservedHeight: Math.max(1, Math.round(baseMetrics.reservedHeight * factor)),
    isVisible: true,
  }
}

export const useLabelStore = defineStore('label', () => {
  const defaultText = ref('')
  const runtimeText = ref('')
  const runtimeTitle = ref('')
  const runtimeState = ref<RuntimeLabelState>('idle')
  const runtimeSource = ref<RuntimeLabelSource>('static')
  const size = ref<LabelSize>('medium')
  const showSessionTitle = ref(true)
  const collapseLongText = ref(true)
  const showDetailedContent = ref(true)
  const catStore = useCatStore()

  const isRuntimeActive = computed(() => runtimeText.value.trim().length > 0)
  const normalizedRuntimeText = computed(() => normalizeLabelText(runtimeText.value))
  const normalizedDefaultText = computed(() => defaultText.value.trim())
  const fullBodyText = computed(() => isRuntimeActive.value ? normalizedRuntimeText.value : normalizedDefaultText.value)
  const displayTitle = computed(() => {
    if (!isRuntimeActive.value || runtimeSource.value === 'signal' || !showSessionTitle.value) {
      return ''
    }

    const title = buildSessionTitle(runtimeTitle.value)

    return title ? `[${title}]` : ''
  })
  const shouldCollapse = computed(() => {
    if (!isRuntimeActive.value || !collapseLongText.value) {
      return false
    }

    return fullBodyText.value.includes('\n') || flattenLabelText(fullBodyText.value).length > LABEL_COLLAPSE_LIMIT
  })
  const displayBody = computed(() => {
    if (!fullBodyText.value) {
      return ''
    }

    const flattened = flattenLabelText(fullBodyText.value)

    if (!shouldCollapse.value) {
      return flattened
    }

    return truncateLabelText(flattened, LABEL_COLLAPSE_LIMIT)
  })
  const displayBodyTag = computed(() => {
    if (!displayBody.value) {
      return ''
    }

    return displayTitle.value ? `[${displayBody.value}]` : displayBody.value
  })
  const tooltipText = computed(() => {
    if (!fullBodyText.value) {
      return ''
    }

    const body = flattenLabelText(fullBodyText.value)

    return displayTitle.value ? `${displayTitle.value}[${body}]` : body
  })
  const displayText = computed(() => `${displayTitle.value}${displayBodyTag.value}`.trim())
  const hasText = computed(() => displayText.value.length > 0)
  const metrics = computed(() => getLabelMetrics(displayText.value, size.value, catStore.window.scale))

  function setDefaultText(text: string) {
    defaultText.value = text
  }

  function setRuntimeLabel(payload: RuntimeLabelPayload) {
    runtimeText.value = payload.text
    runtimeTitle.value = payload.sessionTitle?.trim() ?? ''
    runtimeState.value = payload.state
    runtimeSource.value = payload.source
  }

  function clearRuntimeLabel(nextState: RuntimeLabelState = 'idle') {
    runtimeText.value = ''
    runtimeTitle.value = ''
    runtimeState.value = nextState
    runtimeSource.value = 'static'
  }

  return {
    defaultText,
    runtimeText,
    runtimeTitle,
    runtimeState,
    runtimeSource,
    displayText,
    displayTitle,
    displayBody,
    displayBodyTag,
    tooltipText,
    size,
    showSessionTitle,
    collapseLongText,
    showDetailedContent,
    hasText,
    metrics,
    shouldCollapse,
    setDefaultText,
    setRuntimeLabel,
    clearRuntimeLabel,
  }
}, {
  tauri: {
    filterKeys: ['runtimeText', 'runtimeTitle', 'runtimeState', 'runtimeSource'],
  },
})
