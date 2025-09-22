<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Flex } from 'ant-design-vue'
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import About from './components/about/index.vue'
import Cat from './components/cat/index.vue'
import General from './components/general/index.vue'
import Model from './components/model/index.vue'
import Shortcut from './components/shortcut/index.vue'

import UpdateApp from '@/components/update-app/index.vue'
import { useTray } from '@/composables/useTray'
import { useAppStore } from '@/stores/app'
import { useGeneralStore } from '@/stores/general'
import { isMac } from '@/utils/platform'

const { createTray } = useTray()
const appStore = useAppStore()
const current = ref(0)
const { t } = useI18n()
const generalStore = useGeneralStore()
const appWindow = getCurrentWebviewWindow()

onMounted(async () => {
  createTray()
})

watch(() => generalStore.appearance.language, () => {
  appWindow.setTitle(t('pages.preference.title'))
}, { immediate: true })

const menus = computed(() => [
  {
    label: t('pages.preference.cat.title'),
    icon: 'i-solar:cat-bold',
    component: Cat,
  },
  {
    label: t('pages.preference.general.title'),
    icon: 'i-solar:settings-minimalistic-bold',
    component: General,
  },
  {
    label: t('pages.preference.model.title'),
    icon: 'i-solar:magic-stick-3-bold',
    component: Model,
  },
  {
    label: t('pages.preference.shortcut.title'),
    icon: 'i-solar:keyboard-bold',
    component: Shortcut,
  },
  {
    label: t('pages.preference.about.title'),
    icon: 'i-solar:info-circle-bold',
    component: About,
  },
])
</script>

<template>
  <Flex class="h-screen">
    <div
      class="h-full w-30 flex flex-col items-center gap-4 overflow-auto dark:(bg-color-3 bg-none) bg-gradient-from-primary-1 bg-gradient-to-black/1 bg-gradient-linear"
      :class="[isMac ? 'pt-8' : 'pt-4']"
      data-tauri-drag-region
    >
      <div class="flex flex-col items-center gap-2">
        <div class="b b-color-2 rounded-2xl b-solid">
          <img
            class="size-15"
            data-tauri-drag-region
            src="/logo.png"
          >
        </div>

        <span class="font-bold">{{ appStore.name }}</span>
      </div>

      <div class="flex flex-col gap-2">
        <div
          v-for="(item, index) in menus"
          :key="item.label"
          class="size-20 flex flex-col cursor-pointer items-center justify-center gap-2 rounded-lg hover:bg-color-7 dark:text-color-2 text-color-3 transition"
          :class="{ 'bg-color-2! text-primary-5 dark:text-primary-7 font-bold dark:bg-color-8!': current === index }"
          @click="current = index"
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
      class="flex-1 overflow-auto bg-color-8 dark:bg-color-2 p-4"
      data-tauri-drag-region
    >
      <component :is="item.component" />
    </div>
  </Flex>

  <UpdateApp />
</template>
