<script setup lang="ts">
import { openUrl } from '@tauri-apps/plugin-opener'
import { Button } from 'ant-design-vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { GITHUB_LINK } from '@/constants'
import { useAppStore } from '@/stores/app'

const appStore = useAppStore()

function feedbackIssue() {
  openUrl(`${GITHUB_LINK}/issues/new`)
}
</script>

<template>
  <ProList title="关于软件">
    <ProListItem :description="`版本：v${appStore.version}`" :title="appStore.name">
      <Button type="primary">
        检查更新
      </Button>

      <template #icon>
        <img class="size-12 drop-shadow" src="/images/logo.png">
      </template>
    </ProListItem>

    <ProListItem title="开源地址">
      <Button danger @click="feedbackIssue">
        反馈问题
      </Button>

      <template #description>
        <a :href="GITHUB_LINK" target="_blank">
          {{ GITHUB_LINK }}
        </a>
      </template>
    </ProListItem>
  </ProList>
</template>
