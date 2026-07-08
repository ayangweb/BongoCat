<script setup lang="ts">
import { Button, ColorPicker, Flex, Input, InputNumber, InputPassword, Slider, SpaceAddon, SpaceCompact, Switch } from 'antdv-next'
import { ref } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { say } from '@/composables/useChat'
import { useChatStore } from '@/stores/chat'

import HistoryModal from './components/history-modal/index.vue'

const chatStore = useChatStore()
const testText = ref('你好呀~')
const historyVisible = ref(false)

function handleTest() {
  say(testText.value)
}
</script>

<template>
  <ProList :title="$t('pages.preference.chat.labels.basic')">
    <ProListItem
      :description="$t('pages.preference.chat.hints.enabled')"
      :title="$t('pages.preference.chat.labels.enabled')"
    >
      <Switch v-model:checked="chatStore.ai.enabled" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.chat.hints.history')"
      :title="$t('pages.preference.chat.labels.history')"
    >
      <Button @click="historyVisible = true">
        {{ $t('pages.preference.chat.history.view') }}
      </Button>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.chat.labels.duration')">
      <SpaceCompact>
        <InputNumber
          v-model:value="chatStore.ai.duration"
          class="w-20"
          :min="0"
        />

        <SpaceAddon>s</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.chat.labels.textColor')">
      <ColorPicker
        v-model:value="chatStore.ai.textColor"
        value-format="hex"
      />
    </ProListItem>

    <ProListItem :title="$t('pages.preference.chat.labels.fontSize')">
      <SpaceCompact>
        <InputNumber
          v-model:value="chatStore.ai.fontSize"
          class="w-20"
          :max="64"
          :min="8"
        />

        <SpaceAddon>px</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.chat.labels.bgColor')">
      <ColorPicker
        v-model:value="chatStore.ai.bgColor"
        value-format="hex"
      />
    </ProListItem>

    <ProListItem
      :title="$t('pages.preference.chat.labels.bgOpacity')"
      vertical
    >
      <Slider
        v-model:value="chatStore.ai.bgOpacity"
        class="m-0!"
        :max="100"
        :min="0"
        :tooltip="{
          formatter(value) {
            return `${value}%`
          },
        }"
      />
    </ProListItem>
  </ProList>

  <ProList :title="$t('pages.preference.chat.labels.http')">
    <ProListItem
      :description="$t('pages.preference.chat.hints.http')"
      :title="$t('pages.preference.chat.labels.httpEnabled')"
    >
      <Switch v-model:checked="chatStore.ai.httpEnabled" />
    </ProListItem>

    <template v-if="chatStore.ai.httpEnabled">
      <ProListItem
        :description="$t('pages.preference.chat.hints.httpRestart')"
        :title="$t('pages.preference.chat.labels.httpPort')"
      >
        <InputNumber
          v-model:value="chatStore.ai.httpPort"
          class="w-28"
          :max="65535"
          :min="1024"
        />
      </ProListItem>

      <ProListItem :title="$t('pages.preference.chat.labels.httpToken')">
        <InputPassword
          v-model:value="chatStore.ai.httpToken"
          class="w-48"
        />
      </ProListItem>

      <ProListItem
        :description="$t('pages.preference.chat.hints.httpDocs')"
        :title="$t('pages.preference.chat.labels.httpDocs')"
        vertical
      >
        <Flex
          class="w-full text-3 color-text-tertiary"
          :gap="8"
          vertical
        >
          <div>
            <div class="color-text-secondary">
              {{ $t('pages.preference.chat.labels.basic') }} · /say
            </div>
            <code class="select-all break-all">curl "http://127.0.0.1:{{ chatStore.ai.httpPort }}/say?text=hi&textColor=%23ff0000&fontSize=20&duration=5"</code>
          </div>

          <div>
            <div class="color-text-secondary">
              {{ $t('pages.preference.chat.labels.http') }} · /config
            </div>
            <code class="select-all break-all">curl "http://127.0.0.1:{{ chatStore.ai.httpPort }}/config?bgColor=%23000000&bgOpacity=80"</code>
          </div>

          <div class="break-all">
            text · token? · duration · textColor · fontSize · bgColor · bgOpacity
          </div>
        </Flex>
      </ProListItem>
    </template>
  </ProList>

  <ProList :title="$t('pages.preference.chat.labels.debug')">
    <ProListItem
      :description="$t('pages.preference.chat.hints.debug')"
      :title="$t('pages.preference.chat.labels.debug')"
    >
      <Switch v-model:checked="chatStore.ai.debug" />
    </ProListItem>

    <ProListItem
      v-if="chatStore.ai.debug"
      :title="$t('pages.preference.chat.labels.testText')"
    >
      <Flex :gap="8">
        <Input
          v-model:value="testText"
          class="w-48"
          @press-enter="handleTest"
        />

        <Button
          type="primary"
          @click="handleTest"
        >
          {{ $t('pages.preference.chat.labels.testShow') }}
        </Button>
      </Flex>
    </ProListItem>
  </ProList>

  <HistoryModal v-model="historyVisible" />
</template>
