<script setup lang="ts">
import type { Model } from '@/stores/model'

import { convertFileSrc } from '@tauri-apps/api/core'
import { remove } from '@tauri-apps/plugin-fs'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { Card, Col, message, Popconfirm, Row } from 'ant-design-vue'

import Upload from './components/upload/index.vue'

import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'

const modelStore = useModelStore()

async function handleDelete(item: Model) {
  const { id, path } = item

  await remove(path, { recursive: true })

  modelStore.models = modelStore.models.filter(item => item.id !== id)

  if (id === modelStore.currentModel?.id) {
    modelStore.currentModel = modelStore.models[0]
  }

  message.success('删除成功')
}
</script>

<template>
  <Row :gutter="[16, 16]">
    <Col :span="8">
      <Upload />
    </Col>

    <Col
      v-for="item in modelStore.models"
      :key="item.id"
      :span="8"
    >
      <Card
        hoverable
        size="small"
      >
        <template #cover>
          <img
            alt="example"
            :src="convertFileSrc(join(item.path, 'resources', 'preview.png'))"
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
</template>
