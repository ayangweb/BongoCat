<script setup lang="ts">
import { Flex } from 'ant-design-vue'

const { title } = defineProps<{
  title: string
}>()
</script>

<template>
  <Flex class="mb-4" gap="small" vertical>
    <div class="text-base font-medium" data-tauri-drag-region>
      {{ title }}
    </div>

    <Flex gap="middle" vertical>
      <slot />
    </Flex>
  </FLex>
</template>
