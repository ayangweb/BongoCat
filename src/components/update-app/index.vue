<script setup lang="ts">
import type { Update } from '@tauri-apps/plugin-updater'

import { relaunch } from '@tauri-apps/plugin-process'
import { check } from '@tauri-apps/plugin-updater'
import { useIntervalFn } from '@vueuse/core'
import { Flex, message, Modal } from 'ant-design-vue'
import dayjs from 'dayjs'
import utc from 'dayjs/plugin/utc'
import { computed, reactive, watch } from 'vue'
import VueMarkdown from 'vue-markdown-render'

import { useTauriListen } from '@/composables/useTauriListen'
import { GITHUB_LINK, LISTEN_KEY, UPGRADE_LINK_ACCESS_KEY } from '@/constants'
import { showWindow } from '@/plugins/window'
import { useGeneralStore } from '@/stores/general'

dayjs.extend(utc)

interface State {
  open: boolean
  update?: Update
  downloading: boolean
  totalProgress?: number
  downloadProgress: number
}

const generalStore = useGeneralStore()
const state = reactive<State>({
  open: false,
  downloading: false,
  downloadProgress: 0,
})
const MESSAGE_KEY = 'updatable'

const { pause, resume } = useIntervalFn(checkUpdate, 1000 * 60 * 60 * 24)

watch(() => generalStore.autoCheckUpdate, (value) => {
  pause()

  if (!value) return

  checkUpdate()

  resume()
}, { immediate: true })

useTauriListen<boolean>(LISTEN_KEY.UPDATE_APP, () => {
  checkUpdate(true)

  message.loading({
    key: MESSAGE_KEY,
    duration: 0,
    content: 'Kiểm tra cập nhật...', // Checking for updates-正在检查更新
  })
})

const downloadProgress = computed(() => {
  const { downloadProgress, totalProgress } = state

  if (!totalProgress) return '0%'

  const progress = ((downloadProgress / totalProgress) * 100).toFixed(2)

  return `${progress}%`
})

async function checkUpdate(visibleMessage = false) {
  try {
    const update = await check({
      timeout: 5000,
      headers: {
        'X-AccessKey': UPGRADE_LINK_ACCESS_KEY,
      },
    })

    if (update) {
      const { version, currentVersion, body = '', date, downloadAndInstall } = update

      state.update = Object.assign(update, {
        version: `v${version}`,
        currentVersion: `v${currentVersion}`,
        body: replaceBody(body),
        date: dayjs.utc(date?.split('.')[0]).local().format('YYYY-MM-DD HH:mm:ss'),
        downloadAndInstall: downloadAndInstall.bind(update),
      })

      showWindow()

      state.open = true

      message.destroy(MESSAGE_KEY)
    } else if (visibleMessage) {
      message.success({ key: MESSAGE_KEY, content: 'Hiện là phiên bản mới nhất 🎉' })// It is currently the latest version - 当前已是最新版本
    }
  } catch (error) {
    if (!visibleMessage) return

    message.error({ key: MESSAGE_KEY, content: String(error) })
  }
}

function replaceBody(body: string) {
  return body
    .replace(/&nbsp;/g, '')
    .split('\n')
    .map(line => line.replace(/\s*-\s+by\s+@.*/, ''))
    .join('\n')
}

async function handleOk() {
  try {
    state.downloading = true

    await state.update?.downloadAndInstall((progress) => {
      switch (progress.event) {
        case 'Started':
          state.totalProgress = progress.data.contentLength ?? 0
          break
        case 'Progress':
          state.downloadProgress += progress.data.chunkLength
          break
      }
    })

    relaunch()
  } catch (error) {
    message.error(String(error))
  } finally {
    Object.assign(state, {
      downloading: false,
      downloadProgress: 0,
    })
  }
}
</script>

<template>
  <!--  Update later  稍后更新    Discover a new version 发现新版本 -->
  <Modal
    v-model:open="state.open"
    cancel-text="Cập nhật sau"
    centered
    :closable="false"
    :mask-closable="false"
    title="Khám phá một phiên bản mới🥳"
    @ok="handleOk"
  >
    <!-- 立即更新   Update now -->
    <template #okText>
      {{ state.downloading ? downloadProgress : "Cập nhật ngay bây giờ" }}
    </template>

    <Flex
      class="pt-1"
      gap="small"
      vertical
    >
      <!--   Updated version:   更新版本： -->
      <Flex align="center">
        <span>Phiên bản cập nhật:</span>
        <span>
          <span>{{ state.update?.currentVersion }} 👉 </span>
          <a
            :href="`${GITHUB_LINK}/releases/tag/${state.update?.version}`"
          >
            {{ state.update?.version }}
          </a>
        </span>
      </Flex>

      <Flex align="center">
        <!--  Update time:      更新时间： -->
        <span>Thời gian cập nhật:</span>
        <span>{{ state.update?.date }}</span>
      </Flex>
      <!-- 更新日志： Update log: -->
      <Flex vertical>
        <span>Nhật ký cập nhật:</span>

        <VueMarkdown
          class="update-note max-h-40 overflow-auto"
          :source="state.update?.body ?? ''"
        />
      </Flex>
    </Flex>
  </Modal>
</template>

<style lang="scss" scoped>
.update-note {
  :not(a) {
    all: revert;
  }
}
</style>
