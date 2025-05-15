<script setup lang="ts">
import { emit } from '@tauri-apps/api/event'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Button } from 'ant-design-vue'
import { useI18n } from 'vue-i18n'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { GITHUB_LINK, LISTEN_KEY } from '@/constants'
import { useAppStore } from '@/stores/app'

const { t } = useI18n()
const appStore = useAppStore()

function handleUpdate() {
  emit(LISTEN_KEY.UPDATE_APP)
}

function feedbackIssue() {
  openUrl(`${GITHUB_LINK}/issues/new`)
}
</script>

<template>
  <ProList :title="t('about.title')">
    <ProListItem
      :description="t('about.version', { version: appStore.version })"
      :title="appStore.name"
    >
      <Button
        type="primary"
        @click="handleUpdate"
      >
        {{ t('about.checkUpdate') }}
      </Button>

      <template #icon>
        <div class="b b-color-2 rounded-xl b-solid">
          <img
            class="size-12"
            src="/images/logo.png"
          >
        </div>
      </template>
    </ProListItem>

    <ProListItem :title="t('about.openSource')">
      <Button
        danger
        @click="feedbackIssue"
      >
        {{ t('about.feedback') }}
      </Button>

      <template #description>
        <a :href="GITHUB_LINK">
          {{ GITHUB_LINK }}
        </a>
      </template>
    </ProListItem>
  </ProList>
</template>
