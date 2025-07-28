<script setup lang="ts">
import type { Model } from '@/stores/model'
import type { ComponentPublicInstance } from 'vue'

import { convertFileSrc } from '@tauri-apps/api/core'
import { remove } from '@tauri-apps/plugin-fs'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { useElementSize } from '@vueuse/core'
import { Card, message, Popconfirm } from 'ant-design-vue'
import { ref } from 'vue'
import { MasonryGrid, MasonryGridItem } from 'vue3-masonry-css'

import FloatMenu from './components/float-menu/index.vue'
import Upload from './components/upload/index.vue'

import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'

const modelStore = useModelStore()
const firstItemRef = ref<HTMLElement>()

const { height } = useElementSize(firstItemRef)

function setFirstItemRef(el: Element | ComponentPublicInstance | null, index: number) {
  if (!el || index > 0) return

  if ('$el' in el) {
    return firstItemRef.value = el.$el
  }

  if (el instanceof HTMLElement) {
    firstItemRef.value = el
  }
}

async function handleDelete(item: Model) {
  const { id, path } = item

  try {
    await remove(path, { recursive: true })
    // 删除成功 Delete successfully
    message.success('Xóa thành công')
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
  <MasonryGrid
    :columns="{ 992: 3, 1200: 4, 1600: 6, default: 8 }"
    :gutter="16"
  >
    <MasonryGridItem>
      <Upload :style="{ height: `${height}px` }" />
    </MasonryGridItem>

    <MasonryGridItem
      v-for="(item, index) in modelStore.models"
      :key="item.id"
    >
      <Card
        :ref="(el) => setFirstItemRef(el, index)"
        hoverable
        size="small"
        @click="modelStore.currentModel = item"
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
          />

          <i
            class="i-iconamoon:link-external-bold text-4"
            @click.stop="revealItemInDir(item.path)"
          />

          <template v-if="!item.isPreset">
            <!--  你确定要删除此模型吗？      Are you sure you want to delete this model?    -->
            <!--        删除模型    Delete the model -->
            <Popconfirm
              description="Bạn có chắc là bạn muốn xóa mô hình này?"
              placement="topRight"
              title="Xóa mô hình"
              @confirm="handleDelete(item)"
            >
              <i
                class="i-iconamoon:trash-simple-bold text-4"
                @click.stop
              />
            </Popconfirm>
          </template>
        </template>
      </Card>
    </MasonryGridItem>
  </MasonryGrid>
  <FloatMenu />
</template>
