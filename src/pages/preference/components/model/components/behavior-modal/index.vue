<script setup lang="ts">
import type { MotionInfo } from 'easy-live2d'

import { emit } from '@tauri-apps/api/event'
import { Empty, Modal, Segmented } from 'antdv-next'
import { isEmpty } from 'es-toolkit/compat'
import { ref } from 'vue'

import { LISTEN_KEY } from '@/constants'
import { useModelStore } from '@/stores/model'

import BehaviorItem from './components/behavior-item/index.vue'

const modelValue = defineModel<boolean>()
const modelStore = useModelStore()
const value = ref<'motion' | 'expression'>('motion')

function getMotionShortcutId(groupName: string, index: number) {
  return `${modelStore.currentModel?.id}:motion:${groupName}:${index}`
}

function getExpressionShortcutId(index: number) {
  return `${modelStore.currentModel?.id}:expression:${index}`
}

function startMotion(motion: MotionInfo) {
  emit(LISTEN_KEY.START_MOTION, motion)
}

function setExpression(index: number) {
  emit(LISTEN_KEY.SET_EXPRESSION, index)
}
</script>

<template>
  <Modal
    v-model:open="modelValue"
    :cancel-text="false"
    centered
    :footer="null"
    force-render
    :title="$t('pages.preference.model.behaviorModal.title')"
  >
    <Segmented
      v-model:value="value"
      block
      class="mb-4"
      :options="[
        { label: $t('pages.preference.model.behaviorModal.labels.motion'), value: 'motion' },
        { label: $t('pages.preference.model.behaviorModal.labels.expression'), value: 'expression' },
      ]"
    />

    <div
      v-show="value === 'motion'"
      class="flex flex-col gap-4"
    >
      <Empty
        v-if="isEmpty(modelStore.currentMotions)"
        :image="Empty.PRESENTED_IMAGE_SIMPLE"
      />

      <template v-else>
        <div
          v-for="([groupName, motions], groupIndex) in modelStore.currentMotions"
          :key="groupName"
        >
          <div class="mb-2">
            {{ $t('pages.preference.model.behaviorModal.labels.motionGroupIndex', { index: groupIndex + 1 }) }}
          </div>

          <div class="b-1 b-solid b-border rounded-lg">
            <template
              v-for="(item, index) in motions"
              :key="item.no"
            >
              <BehaviorItem
                v-model="modelStore.shortcuts[getMotionShortcutId(groupName, index)]"
                :label="$t('pages.preference.model.behaviorModal.labels.motionIndex', { index: index + 1 })"
                @click="startMotion(item)"
              />
            </template>
          </div>
        </div>
      </template>
    </div>

    <div
      v-show="value === 'expression'"
      class="flex flex-col"
    >
      <Empty
        v-if="isEmpty(modelStore.currentExpressions)"
        :image="Empty.PRESENTED_IMAGE_SIMPLE"
      />

      <div class="b-1 b-solid b-border rounded-lg">
        <template
          v-for="(item, index) in modelStore.currentExpressions"
          :key="item.name"
        >
          <BehaviorItem
            v-model="modelStore.shortcuts[getExpressionShortcutId(index)]"
            :label="$t('pages.preference.model.behaviorModal.labels.expressionIndex', { index: index + 1 })"
            @click="setExpression(index)"
          />
        </template>
      </div>
    </div>
  </Modal>
</template>
