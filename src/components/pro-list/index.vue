<script setup lang="ts">
import { Flex } from 'ant-design-vue'

const { title } = defineProps<{
  title: string
}>()
</script>

<template>
  <Flex
    class="not-last:mb-4"
    gap="small"
    vertical
  >
    <div
      class="text-4 font-medium"
      data-tauri-drag-region
    >
      {{ title }}
    </div>

    <Flex
      gap="middle"
      vertical
    >
      <slot />
    </Flex>
  </FLex>
</template>
