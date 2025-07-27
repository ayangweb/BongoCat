<script setup lang="ts">
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
import { Switch } from 'ant-design-vue'
import { watch } from 'vue'

import MacosPermissions from './components/macos-permissions/index.vue'
import ThemeMode from './components/theme-mode/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useGeneralStore } from '@/stores/general'

const generalStore = useGeneralStore()

watch(() => generalStore.autostart, async (value) => {
  const enabled = await isEnabled()

  if (value && !enabled) {
    return enable()
  }

  if (!value && enabled) {
    disable()
  }
}, { immediate: true })
</script>

<template>
  <MacosPermissions />
  <!-- 应用设置 Apply settings -->
  <!-- Start up automatically 开机自启动 -->
  <ProList title="Áp dụng cài đặt">
    <ProListItem title="Khởi động tự động">
      <Switch v-model:checked="generalStore.autostart" />
    </ProListItem>
    <!-- 启用后，即可通过 OBS Studio 捕获窗口。Once enabled, the window can be captured through OBS Studio. -->
    <!-- 显示任务栏图标 Show taskbar icon -->
    <ProListItem
      description="Sau khi được bật, cửa sổ có thể được chụp thông qua OBS Studio."
      title="Hiển thị biểu tượng thanh tác vụ"
    >
      <Switch v-model:checked="generalStore.taskbarVisibility" />
    </ProListItem>
  </ProList>
  <!-- 外观设置 Appearance settings -->
  <ProList title="Cài đặt ngoại hình">
    <ThemeMode />
  </ProList>
  <!-- 更新设置Update settings -->
  <!-- Automatically check for updates  自动检查更新 -->
  <ProList title="Cập nhật cài đặt">
    <ProListItem title="Tự động kiểm tra các bản cập nhật">
      <Switch v-model:checked="generalStore.autoCheckUpdate" />
    </ProListItem>
  </ProList>
</template>
