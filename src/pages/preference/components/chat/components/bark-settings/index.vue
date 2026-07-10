<script setup lang="ts">
import { Button, Flex, Input, InputPassword, Select, Switch } from 'antdv-next'
import { ref } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { useBark } from '@/composables/useBark'
import { useChatStore } from '@/stores/chat'

const chatStore = useChatStore()
const { register } = useBark()
const registering = ref(false)
const registerError = ref('')

const modeOptions = [
  { value: 'cbc', label: 'AES-CBC' },
  { value: 'gcm', label: 'AES-GCM' },
]

async function handleRegister() {
  registering.value = true
  registerError.value = ''

  try {
    await register()
  } catch (error) {
    registerError.value = (error as Error).message ?? String(error)
  } finally {
    registering.value = false
  }
}
</script>

<template>
  <ProList :title="$t('pages.preference.chat.labels.bark')">
    <ProListItem
      :description="$t('pages.preference.chat.hints.bark')"
      :title="$t('pages.preference.chat.labels.barkEnabled')"
    >
      <Switch v-model:checked="chatStore.bark.enabled" />
    </ProListItem>

    <template v-if="chatStore.bark.enabled">
      <ProListItem :title="$t('pages.preference.chat.labels.barkServerUrl')">
        <Input
          v-model:value="chatStore.bark.serverUrl"
          class="w-64"
          placeholder="https://bark.example.com"
        />
      </ProListItem>

      <ProListItem
        :description="$t('pages.preference.chat.hints.barkRegister')"
        :title="$t('pages.preference.chat.labels.barkRegister')"
      >
        <Flex
          align="center"
          :gap="8"
        >
          <span
            v-if="registerError"
            class="text-3 color-red"
          >{{ registerError }}</span>

          <Button
            :disabled="!chatStore.bark.serverUrl"
            :loading="registering"
            @click="handleRegister"
          >
            {{ $t('pages.preference.chat.labels.barkRegister') }}
          </Button>
        </Flex>
      </ProListItem>

      <ProListItem
        v-if="chatStore.bark.deviceKey"
        :title="$t('pages.preference.chat.labels.barkDeviceKey')"
      >
        <code class="select-all break-all text-3">{{ chatStore.bark.deviceKey }}</code>
      </ProListItem>

      <ProListItem :title="$t('pages.preference.chat.labels.barkStatus')">
        {{ $t(`pages.preference.chat.barkStatus.${chatStore.barkStatus}`) }}
      </ProListItem>

      <ProListItem
        :description="$t('pages.preference.chat.hints.barkCrypto')"
        :title="$t('pages.preference.chat.labels.barkCryptoKey')"
      >
        <Flex :gap="8">
          <Select
            v-model:value="chatStore.bark.cryptoMode"
            class="w-30"
            :options="modeOptions"
          />

          <InputPassword
            v-model:value="chatStore.bark.cryptoKey"
            class="w-40"
          />

          <Input
            v-model:value="chatStore.bark.cryptoIv"
            class="w-36"
            :placeholder="$t('pages.preference.chat.labels.barkCryptoIv')"
          />
        </Flex>
      </ProListItem>
    </template>
  </ProList>
</template>
