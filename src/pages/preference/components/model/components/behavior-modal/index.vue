<script setup lang="ts">
import type { MotionInfo } from 'easy-live2d'

import { emit } from '@tauri-apps/api/event'
import { Empty, List, Modal, Segmented } from 'ant-design-vue'
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

          <List
            bordered
            :data-source="motions"
            size="small"
          >
            <template #renderItem="{ index }">
              <BehaviorItem
                v-model="modelStore.shortcuts[getMotionShortcutId(groupName, index)]"
                :label="$t('pages.preference.model.behaviorModal.labels.motionIndex', { index: index + 1 })"
                @click="startMotion(motions[index])"
              />
            </template>
          </List>
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

      <List
        v-else
        bordered
        :data-source="modelStore.currentExpressions"
        size="small"
      >
        <template #renderItem="{ index }">
          <BehaviorItem
            v-model="modelStore.shortcuts[getExpressionShortcutId(index)]"
            :label="$t('pages.preference.model.behaviorModal.labels.expressionIndex', { index: index + 1 })"
            @click="setExpression(index)"
          />
        </template>
      </List>
    </div>
  </Modal>
</template>
