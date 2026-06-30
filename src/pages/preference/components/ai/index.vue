<script setup lang="ts">
import { Button, ColorPicker, Flex, Input, InputNumber, InputPassword, Slider, SpaceAddon, SpaceCompact, Switch } from 'antdv-next'
import { ref } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { say } from '@/composables/useChat'
import { useAiStore } from '@/stores/ai'

const aiStore = useAiStore()
const testText = ref('你好呀~')

function handleTest() {
  say(testText.value)
}
</script>

<template>
  <ProList :title="$t('pages.preference.ai.labels.basic')">
    <ProListItem
      :description="$t('pages.preference.ai.hints.enabled')"
      :title="$t('pages.preference.ai.labels.enabled')"
    >
      <Switch v-model:checked="aiStore.ai.enabled" />
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.duration')">
      <SpaceCompact>
        <InputNumber
          v-model:value="aiStore.ai.duration"
          class="w-20"
          :min="0"
        />

        <SpaceAddon>s</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.textColor')">
      <ColorPicker v-model:value="aiStore.ai.textColor" />
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.fontSize')">
      <SpaceCompact>
        <InputNumber
          v-model:value="aiStore.ai.fontSize"
          class="w-20"
          :max="64"
          :min="8"
        />

        <SpaceAddon>px</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.bgColor')">
      <ColorPicker v-model:value="aiStore.ai.bgColor" />
    </ProListItem>

    <ProListItem
      :title="$t('pages.preference.ai.labels.bgOpacity')"
      vertical
    >
      <Slider
        v-model:value="aiStore.ai.bgOpacity"
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

  <ProList :title="$t('pages.preference.ai.labels.http')">
    <ProListItem
      :description="$t('pages.preference.ai.hints.http')"
      :title="$t('pages.preference.ai.labels.httpEnabled')"
    >
      <Switch v-model:checked="aiStore.ai.httpEnabled" />
    </ProListItem>

    <template v-if="aiStore.ai.httpEnabled">
      <ProListItem
        :description="$t('pages.preference.ai.hints.httpRestart')"
        :title="$t('pages.preference.ai.labels.httpPort')"
      >
        <InputNumber
          v-model:value="aiStore.ai.httpPort"
          class="w-28"
          :max="65535"
          :min="1024"
        />
      </ProListItem>

      <ProListItem :title="$t('pages.preference.ai.labels.httpToken')">
        <InputPassword
          v-model:value="aiStore.ai.httpToken"
          class="w-48"
        />
      </ProListItem>

      <ProListItem
        :description="$t('pages.preference.ai.hints.httpDocs')"
        :title="$t('pages.preference.ai.labels.httpDocs')"
        vertical
      >
        <Flex
          class="w-full text-3 color-text-tertiary"
          :gap="8"
          vertical
        >
          <div>
            <div class="color-text-secondary">
              {{ $t('pages.preference.ai.labels.basic') }} · /say
            </div>
            <code class="select-all break-all">curl "http://127.0.0.1:{{ aiStore.ai.httpPort }}/say?text=hi&textColor=%23ff0000&fontSize=20&duration=5"</code>
          </div>

          <div>
            <div class="color-text-secondary">
              {{ $t('pages.preference.ai.labels.http') }} · /config
            </div>
            <code class="select-all break-all">curl "http://127.0.0.1:{{ aiStore.ai.httpPort }}/config?bgColor=%23000000&bgOpacity=80"</code>
          </div>

          <div class="break-all">
            text · token? · duration · textColor · fontSize · bgColor · bgOpacity
          </div>
        </Flex>
      </ProListItem>
    </template>
  </ProList>

  <ProList :title="$t('pages.preference.ai.labels.debug')">
    <ProListItem
      :description="$t('pages.preference.ai.hints.debug')"
      :title="$t('pages.preference.ai.labels.debug')"
    >
      <Switch v-model:checked="aiStore.ai.debug" />
    </ProListItem>

    <ProListItem
      v-if="aiStore.ai.debug"
      :title="$t('pages.preference.ai.labels.testText')"
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
          {{ $t('pages.preference.ai.labels.testShow') }}
        </Button>
      </Flex>
    </ProListItem>
  </ProList>
</template>
