<script setup lang="ts">
import { emit } from '@tauri-apps/api/event'
import { appLogDir } from '@tauri-apps/api/path'
import { openPath, openUrl } from '@tauri-apps/plugin-opener'
import { Button } from 'ant-design-vue'
import { onMounted, ref } from 'vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { GITHUB_LINK, LISTEN_KEY } from '@/constants'
import { useAppStore } from '@/stores/app'

const appStore = useAppStore()
const logDir = ref('')

onMounted(async () => {
  logDir.value = await appLogDir()
})

function handleUpdate() {
  emit(LISTEN_KEY.UPDATE_APP)
}

function feedbackIssue() {
  openUrl(`${GITHUB_LINK}/issues/new`)
}
</script>

<template>
  <ProList title="关于软件">
    <ProListItem
      :description="`版本：v${appStore.version}`"
      :title="appStore.name"
    >
      <Button
        type="primary"
        @click="handleUpdate"
      >
        检查更新
      </Button>

      <template #icon>
        <div class="b b-color-2 rounded-xl b-solid">
          <img
            class="size-12"
            src="/logo.png"
          >
        </div>
      </template>
    </ProListItem>

    <ProListItem title="开源地址">
      <Button
        danger
        @click="feedbackIssue"
      >
        反馈问题
      </Button>

      <template #description>
        <a :href="GITHUB_LINK">
          {{ GITHUB_LINK }}
        </a>
      </template>
    </ProListItem>

    <ProListItem
      :description="logDir"
      title="软件日志"
    >
      <Button @click="openPath(logDir)">
        查看日志
      </Button>
    </ProListItem>
  </ProList>
</template>
