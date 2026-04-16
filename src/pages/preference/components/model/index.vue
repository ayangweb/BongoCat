<script setup lang="ts">
import type { Model } from '@/stores/model'
import type { ComponentPublicInstance } from 'vue'

import { convertFileSrc } from '@tauri-apps/api/core'
import { remove } from '@tauri-apps/plugin-fs'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { useElementSize } from '@vueuse/core'
import { Card, message, Popconfirm } from 'ant-design-vue'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { MasonryGrid, MasonryGridItem } from 'vue3-masonry-css'

import BehaviorModal from './components/behavior-modal/index.vue'
import FloatMenu from './components/float-menu/index.vue'
import Upload from './components/upload/index.vue'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'

const catStore = useCatStore()
const modelStore = useModelStore()
const firstItemRef = ref<HTMLElement>()
const { height } = useElementSize(firstItemRef)
const { t } = useI18n()
const openBehaviorModal = ref(false)

function setFirstItemRef(el: Element | ComponentPublicInstance | null, index: number) {
  if (!el || index > 0) return

  if ('$el' in el) {
    return firstItemRef.value = el.$el
  }

  if (el instanceof HTMLElement) {
    firstItemRef.value = el
  }
}

function handleToggle(nextModel: Model) {
  if (modelStore.currentModel?.id === nextModel.id) return

  modelStore.modelReady = false

  modelStore.currentModel = nextModel
}

async function handleDelete(item: Model) {
  const { id, path } = item

  try {
    await remove(path, { recursive: true })

    message.success(t('pages.preference.model.hints.deleteSuccess'))
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
        class="[&_[class^='i-']]:text-4"
        hoverable
        size="small"
        @click="handleToggle(item)"
      >
        <template #cover>
          <img
            alt="example"
            :src="convertFileSrc(join(item.path, 'resources', 'cover.png'))"
          >
        </template>

        <template #actions>
          <i
            class="i-lucide:circle-check"
            :class="{ 'text-success': item.id === modelStore.currentModel?.id }"
          />

          <i
            v-if="catStore.model.behavior && modelStore.currentModel?.id === item.id"
            class="i-lucide:smile"
            @click.stop="openBehaviorModal = true"
          />

          <i
            class="i-lucide:folder-open"
            @click.stop="revealItemInDir(item.path)"
          />

          <template v-if="!item.isPreset">
            <Popconfirm
              :description="$t('pages.preference.model.hints.deleteModel')"
              placement="topRight"
              :title="$t('pages.preference.model.labels.deleteModel')"
              @confirm="handleDelete(item)"
            >
              <i
                class="i-lucide:trash-2"
                @click.stop
              />
            </Popconfirm>
          </template>
        </template>
      </Card>
    </MasonryGridItem>
  </MasonryGrid>

  <FloatMenu />

  <BehaviorModal
    v-if="catStore.model.behavior"
    v-model="openBehaviorModal"
  />
</template>
