<script setup lang="ts">
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
import { Switch } from 'ant-design-vue'
import { watch } from 'vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useGeneralStore } from '@/stores/general'

const generalStore = useGeneralStore()

watch(
  () => generalStore.autoStart,
  async (newValue) => {
    if (import.meta.env.PROD) {
      const enabled = await isEnabled()
      if (newValue && !enabled) {
        await enable()
      } else if (!newValue && enabled) {
        await disable()
      }
    } else {
      console.warn('Autostart plugin is not available in development mode.')
    }
  },
)
</script>

<template>
  <ProList title="应用设置">
    <ProListItem title="开机自启动">
      <Switch v-model:checked="generalStore.autoStart" />
    </ProListItem>
  </ProList>

  <ProList title="更新设置">
    <ProListItem title="自动检查更新">
      <Switch v-model:checked="generalStore.autoCheckUpdate" />
    </ProListItem>
  </ProList>
</template>
