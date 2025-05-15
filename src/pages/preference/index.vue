<script setup lang="ts">
import { Flex } from 'ant-design-vue'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import About from './components/about/index.vue'
import Cat from './components/cat/index.vue'
import General from './components/general/index.vue'
import Model from './components/model/index.vue'

import UpdateApp from '@/components/update-app/index.vue'
import { useTray } from '@/composables/useTray'
import { useAppStore } from '@/stores/app'
import { isMac } from '@/utils/platform'

const { createTray } = useTray()
const appStore = useAppStore()
const current = ref(0)

onMounted(async () => {
  createTray()
})

const { t } = useI18n()
const menus = [
  {
    label: t('preference.catSettings'),
    icon: 'i-solar:cat-bold',
    component: Cat,
  },
  {
    label: t('preference.generalSettings'),
    icon: 'i-solar:settings-minimalistic-bold',
    component: General,
  },
  {
    label: t('preference.modelManagement'),
    icon: 'i-solar:magic-stick-3-bold',
    component: Model,
  },
  {
    label: t('preference.about'),
    icon: 'i-solar:info-circle-bold',
    component: About,
  },
]
</script>

<template>
  <Flex class="h-screen">
    <div
      class="h-full w-30 flex flex-col items-center gap-4 bg-gradient-from-primary-1 bg-gradient-to-black/1 bg-gradient-linear"
      :class="[isMac ? 'pt-8' : 'pt-4']"
      data-tauri-drag-region
    >
      <div class="flex flex-col items-center gap-2">
        <div class="b b-color-2 rounded-2xl b-solid">
          <img
            class="size-15"
            src="/images/logo.png"
          >
        </div>

        <span class="font-bold">{{ appStore.name }}</span>
      </div>

      <div class="flex flex-col gap-2">
        <div
          v-for="(item, index) in menus"
          :key="item.label"
          class="size-20 flex flex-col cursor-pointer items-center justify-center gap-2 rounded-lg hover:bg-color-7 text-color-3 transition"
          :class="{ 'bg-white! text-primary-5 font-bold': current === index }"
          @mousedown="current = index"
        >
          <div
            class="size-8"
            :class="item.icon"
          />

          <span>{{ item.label }}</span>
        </div>
      </div>
    </div>

    <div
      v-for="(item, index) in menus"
      v-show="current === index"
      :key="item.label"
      class="flex-1 bg-color-8 p-4"
      data-tauri-drag-region
    >
      <component :is="item.component" />
    </div>
  </Flex>

  <UpdateApp />
</template>
