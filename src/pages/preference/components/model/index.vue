<script setup lang="ts">
import type { Model } from '@/stores/model'
import type { ColProps } from 'ant-design-vue'

import { convertFileSrc } from '@tauri-apps/api/core'
import { remove } from '@tauri-apps/plugin-fs'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { Card, Col, InputNumber, message, Popconfirm, Row, Switch } from 'ant-design-vue'
import { ref, watch } from 'vue'

import FloatMenu from './components/float-menu/index.vue'
import Upload from './components/upload/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'

const modelStore = useModelStore()

const colProps: ColProps = {
  xs: 12,
  md: 8,
  lg: 6,
  xl: 4,
}

// 将秒数转换为分秒格式
function secondsToTime(seconds: number) {
  const minutes = Math.floor(seconds / 60)
  const remainingSeconds = seconds % 60
  return { minutes, seconds: remainingSeconds }
}

// 将分秒格式转换为秒数
function timeToSeconds(minutes: number, seconds: number) {
  return minutes * 60 + seconds
}

// 初始化分秒值
const { minutes: initialMinutes, seconds: initialSeconds } = secondsToTime(modelStore.autoSwitchInterval)
const minutes = ref(initialMinutes)
const seconds = ref(initialSeconds)

// 监听 autoSwitchInterval 的变化
watch(() => modelStore.autoSwitchInterval, (newValue) => {
  const { minutes: newMinutes, seconds: newSeconds } = secondsToTime(newValue)
  minutes.value = newMinutes
  seconds.value = newSeconds
})

// 处理时间变化
function handleTimeChange() {
  // 时间变化时不需要额外操作，useDevice 会自动处理
}

async function handleDelete(item: Model) {
  const { id, path } = item

  try {
    await remove(path, { recursive: true })

    message.success('删除成功')
  } catch (error) {
    message.error(String(error))
  } finally {
    modelStore.models = modelStore.models.filter(item => item.id !== id)

    if (id === modelStore.currentModel?.id) {
      modelStore.currentModel = modelStore.models[0]
    }
  }
}
</script>

<template>
  <Row :gutter="[16, 16]">
    <Col v-bind="colProps">
      <Upload />
    </Col>

    <Col
      v-for="item in modelStore.models"
      :key="item.id"
      v-bind="colProps"
    >
      <Card
        hoverable
        size="small"
      >
        <template #cover>
          <img
            alt="example"
            :src="convertFileSrc(join(item.path, 'resources', 'cover.png'))"
          >
        </template>

        <template #actions>
          <i
            class="i-iconamoon:check-circle-1-bold text-4"
            :class="{ 'text-success': item.id === modelStore.currentModel?.id }"
            @click="modelStore.currentModel = item"
          />

          <i
            class="i-iconamoon:link-external-bold text-4"
            @click="revealItemInDir(item.path)"
          />

          <template v-if="!item.isPreset">
            <Popconfirm
              description="你确定要删除此模型吗？"
              placement="topRight"
              title="删除模型"
              @confirm="handleDelete(item)"
            >
              <i class="i-iconamoon:trash-simple-bold text-4" />
            </Popconfirm>
          </template>
        </template>
      </Card>
    </Col>
  </Row>

  <ProList title="自动切换设置">
    <ProListItem
      description="启用后，在未操作一段时间后自动切换到下一个模型"
      title="自动切换模型"
    >
      <Switch
        v-model:checked="modelStore.autoSwitchEnabled"
      />
    </ProListItem>

    <ProListItem
      description="设置自动切换的时间间隔"
      title="切换间隔"
    >
      <div class="flex items-center gap-2">
        <InputNumber
          v-model:value="minutes"
          class="w-20"
          :max="60"
          :min="0"
          :step="1"
          @change="() => {
            modelStore.autoSwitchInterval = timeToSeconds(minutes, seconds);
            handleTimeChange();
          }"
        />
        <span>分</span>
        <InputNumber
          v-model:value="seconds"
          class="w-20"
          :max="59"
          :min="0"
          :step="1"
          @change="() => {
            modelStore.autoSwitchInterval = timeToSeconds(minutes, seconds);
            handleTimeChange();
          }"
        />
        <span>秒</span>
      </div>
    </ProListItem>

    <ProListItem
      description="设置自动切换的目标模型"
      title="目标模型"
    >
      <div class="flex items-center gap-2">
        <InputNumber
          class="w-20"
          :max="modelStore.models.length"
          :min="1"
          placeholder="第n个模型"
          :step="1"
          :value="modelStore.targetModelIndex ?? undefined"
          @update:value="(val) => {
            modelStore.targetModelIndex = typeof val === 'number' ? val : null;
            handleTimeChange();
          }"
        />
        <span>个</span>
      </div>
    </ProListItem>
  </ProList>

  <FloatMenu />
</template>
