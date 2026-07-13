<script setup lang="ts">
import { DateRangePicker, Flex, Modal, Select, Table, Tag } from 'antdv-next'
import dayjs from 'dayjs'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { ChatMessage } from '@/utils/chatHistory'

import { useChatHistoryStore } from '@/stores/chatHistory'
import { filterHistory } from '@/utils/chatHistory'

const modelValue = defineModel<boolean>()
const { t } = useI18n()
const chatHistoryStore = useChatHistoryStore()

const status = ref<ChatMessage['status']>()
const source = ref<ChatMessage['source']>()
const range = ref<[unknown, unknown]>()
const detailText = ref<string>()

const statusOptions = computed(() => [
  { value: 'shown', label: t('pages.preference.chat.history.shown') },
  { value: 'skipped', label: t('pages.preference.chat.history.skipped') },
])

const sourceOptions = computed(() => [
  { value: 'http', label: t('pages.preference.chat.history.http') },
  { value: 'internal', label: t('pages.preference.chat.history.internal') },
  { value: 'bark', label: t('pages.preference.chat.history.bark') },
])

// 日期闭区间：开始日 00:00:00.000 → 结束日 23:59:59.999；dayjs() 对字符串/Dayjs 输入都适用
const rows = computed(() => {
  const [start, end] = range.value ?? []
  const ms: [number, number] | undefined = start && end
    ? [dayjs(start as never).startOf('day').valueOf(), dayjs(end as never).endOf('day').valueOf()]
    : undefined

  return filterHistory(chatHistoryStore.history, {
    status: status.value,
    source: source.value,
    range: ms,
  }).slice().reverse()
})

const columns = computed(() => [
  { title: t('pages.preference.chat.history.time'), key: 'time', width: 170 },
  { title: t('pages.preference.chat.history.status'), key: 'status', width: 90 },
  { title: t('pages.preference.chat.history.source'), key: 'source', width: 90 },
  { title: t('pages.preference.chat.history.content'), dataIndex: 'text', key: 'text', ellipsis: true },
  { title: t('pages.preference.chat.history.action'), key: 'action', width: 80 },
])
</script>

<template>
  <Modal
    v-model:open="modelValue"
    :footer="null"
    :title="$t('pages.preference.chat.labels.history')"
    width="720px"
  >
    <Flex
      class="mb-3"
      :gap="8"
    >
      <Select
        v-model:value="status"
        allow-clear
        class="w-30"
        :options="statusOptions"
        :placeholder="$t('pages.preference.chat.history.filterStatus')"
      />

      <Select
        v-model:value="source"
        allow-clear
        class="w-30"
        :options="sourceOptions"
        :placeholder="$t('pages.preference.chat.history.filterSource')"
      />

      <DateRangePicker
        v-model:value="range"
        allow-clear
      />
    </Flex>

    <Table
      :columns="columns"
      :data-source="rows"
      :pagination="{ pageSize: 20 }"
      :row-key="(_: ChatMessage, index: number) => index"
      size="small"
    >
      <template #bodyCell="{ column, record }">
        <template v-if="column.key === 'time'">
          {{ dayjs(record.time).format('YYYY-MM-DD HH:mm:ss') }}
        </template>

        <template v-else-if="column.key === 'status'">
          <Tag :color="record.status === 'shown' ? 'green' : 'default'">
            {{ $t(`pages.preference.chat.history.${record.status}`) }}
          </Tag>
        </template>

        <template v-else-if="column.key === 'source'">
          {{ $t(`pages.preference.chat.history.${record.source}`) }}
        </template>

        <template v-else-if="column.key === 'action'">
          <a @click="detailText = record.text">
            {{ $t('pages.preference.chat.history.detail') }}
          </a>
        </template>
      </template>
    </Table>

    <Modal
      :footer="null"
      :open="detailText !== undefined"
      :title="$t('pages.preference.chat.history.detail')"
      @cancel="detailText = undefined"
    >
      <div class="max-h-80 overflow-auto whitespace-pre-wrap break-words">
        {{ detailText }}
      </div>
    </Modal>
  </Modal>
</template>
