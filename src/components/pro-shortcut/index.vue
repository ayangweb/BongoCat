<script setup lang="ts">
import type { Key } from '@/utils/keyboard'

import { find, map, remove, some, split } from 'es-toolkit/compat'
import { ref, useTemplateRef, watch } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import { keys, modifierKeys, standardKeys } from '@/utils/keyboard'

const props = defineProps<{
  title: string
  description?: string
}>()

const modelValue = defineModel<string>()
const shortcutInputRef = useTemplateRef('shortcutInput')
const isFocusing = ref(false)
const isHovering = ref(false)
const pressedKeys = ref<Key[]>([])

watch(modelValue, () => {
  parseModelValue()
}, { immediate: true })

function parseModelValue() {
  if (!modelValue.value) {
    return pressedKeys.value = []
  }

  pressedKeys.value = split(modelValue.value, '+').map((tauriKey) => {
    return find(keys, { tauriKey })!
  })
}

function getEventKey(event: KeyboardEvent) {
  const { key, code } = event

  const eventKey = key.replace('Meta', 'Command')

  const isModifierKey = some(modifierKeys, { eventKey })

  return isModifierKey ? eventKey : code
}

function isValidShortcut() {
  if (pressedKeys.value?.[0]?.eventKey?.startsWith('F')) {
    return true
  }

  const hasModifierKey = some(pressedKeys.value, ({ eventKey }) => {
    return some(modifierKeys, { eventKey })
  })
  const hasStandardKey = some(pressedKeys.value, ({ eventKey }) => {
    return some(standardKeys, { eventKey })
  })

  return hasModifierKey && hasStandardKey
}

function handleFocus() {
  isFocusing.value = true

  pressedKeys.value = []
}

function handleBlur() {
  isFocusing.value = false

  if (!isValidShortcut()) {
    return parseModelValue()
  }

  modelValue.value = map(pressedKeys.value, 'tauriKey').join('+')
}

function handleKeyDown(event: KeyboardEvent) {
  const eventKey = getEventKey(event)

  const matched = find(keys, { eventKey })
  const isInvalid = !matched
  const isDuplicate = some(pressedKeys.value, { eventKey })

  if (isInvalid || isDuplicate) return

  pressedKeys.value.push(matched)

  if (isValidShortcut()) {
    shortcutInputRef.value?.blur()
  }
}

function handleKeyUp(event: KeyboardEvent) {
  remove(pressedKeys.value, { eventKey: getEventKey(event) })
}
</script>

<template>
  <ProListItem v-bind="props">
    <div
      ref="shortcutInput"
      align="center"
      class="relative h-8 min-w-32 flex cursor-text items-center justify-center b b-color-1 hover:b-primary-5 rounded-md b-solid px-2.5 text-color-3 outline-none transition focus:(b-primary shadow-[0_0_0_2px_rgba(5,145,255,0.1)])"
      justify="center"
      :tabindex="0"
      @blur="handleBlur"
      @focus="handleFocus"
      @keydown="handleKeyDown"
      @keyup="handleKeyUp"
      @mouseout="isHovering = false"
      @mouseover="isHovering = true"
    >
      <!--  按下录制快捷键 Press the recording shortcut key   点击录制快捷键  Click the recording shortcut key -->
      <span v-if="pressedKeys.length === 0">
        {{ isFocusing ? 'Nhấn phím tắt ghi âm' : 'Nhấp vào phím tắt ghi âm' }}
      </span>

      <span class="text-primary font-bold">
        {{ map(pressedKeys, 'symbol').join(' ') }}
      </span>

      <div
        class="i-iconamoon:close-circle-1 absolute right-2 cursor-pointer text-4 transition hover:text-primary"
        :hidden="isFocusing || !isHovering || pressedKeys.length === 0"
        @mousedown.prevent="modelValue = ''"
      />
    </div>
  </ProListItem>
</template>
