<script setup lang="ts">
import type { Update } from '@tauri-apps/plugin-updater'

import { relaunch } from '@tauri-apps/plugin-process'
import { check } from '@tauri-apps/plugin-updater'
import { useIntervalFn } from '@vueuse/core'
import { Flex, message, Modal } from 'ant-design-vue'
import dayjs from 'dayjs'
import utc from 'dayjs/plugin/utc'
import { computed, onMounted, ref, watch } from 'vue'
import VueMarkdown from 'vue-markdown-render'

import { useTauriListen } from '@/composables/useTauriListen'
import { GITHUB_LINK, LISTEN_KEY } from '@/constants'
import { useGeneralStore } from '@/stores/general'

dayjs.extend(utc)

const generalStore = useGeneralStore()
const open = ref(false)
const updateInfo = ref<Update>()
const loading = ref(false)
const total = ref(0)
const download = ref(0)
const MESSAGE_KEY = 'updatable'

onMounted(() => {
  checkUpdate()
})

const { pause, resume } = useIntervalFn(checkUpdate, 1000 * 60 * 60 * 24, { immediate: true })

watch(() => generalStore.autoCheckUpdate, (value) => {
  pause()

  if (!value) return

  resume()
}, { immediate: true })

useTauriListen<boolean>(LISTEN_KEY.UPDATE_APP, () => {
  checkUpdate(true)

  message.loading({
    key: MESSAGE_KEY,
    duration: 0,
    content: '正在检查更新...',
  })
})

const downloadProgress = computed(() => {
  return ((download.value / total.value) * 100).toFixed(2)
})

async function checkUpdate(visibleMessage = false) {
  try {
    const update = await check()

    if (update) {
      const { version, currentVersion, body = '', date } = update

      updateInfo.value = Object.assign(update, {
        version: `v${version}`,
        currentVersion: `v${currentVersion}`,
        body: replaceBody(body),
        date: dayjs.utc(date?.split('.')[0]).local().format('YYYY-MM-DD HH:mm:ss'),
      })

      open.value = true

      message.destroy(MESSAGE_KEY)
    } else if (visibleMessage) {
      message.success({ key: MESSAGE_KEY, content: '当前已是最新版本🎉' })
    }
  } catch (error) {
    if (!visibleMessage) return

    message.error({ key: MESSAGE_KEY, content: String(error) })
  }
}

function replaceBody(body: string) {
  return body
    .split('\n')
    .map(line => line.replace(/\s*-\s+by\s+@.*/, ''))
    .join('\n')
}

async function handleOk() {
  loading.value = true

  await updateInfo.value?.downloadAndInstall((progress) => {
    switch (progress.event) {
      case 'Started':
        total.value = progress.data.contentLength ?? 0
        break
      case 'Progress':
        download.value += progress.data.chunkLength
        break
    }
  })

  loading.value = false

  relaunch()
}
</script>

<template>
  <Modal v-model:open="open" cancel-text="稍后更新" centered :closable="false" :mask-closable="false" title="发现新版本🥳" @ok="handleOk">
    <template #okText>
      {{ loading ? downloadProgress : "立即更新" }}
    </template>

    <Flex class="pt-1" gap="small" vertical>
      <Flex align="center">
        <span>更新版本：</span>
        <span>
          <span>{{ updateInfo?.currentVersion }} 👉 </span>
          <a
            :href="`${GITHUB_LINK}/releases/tag/${updateInfo?.version}`"
          >
            {{ updateInfo?.version }}
          </a>
        </span>
      </Flex>

      <Flex align="center">
        <span>更新时间：</span>
        <span>{{ updateInfo?.date }}</span>
      </Flex>

      <Flex vertical>
        <span>更新日志：</span>

        <VueMarkdown
          class="mt-2 max-h-40 overflow-auto"
          :source="updateInfo?.body ?? ''"
        />
      </Flex>
    </Flex>
  </Modal>
</template>
