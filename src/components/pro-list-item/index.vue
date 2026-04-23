<script setup lang="ts">
import { Flex } from 'antdv-next'
import { computed } from 'vue'

const { title, description, vertical } = defineProps<{
  title: string
  description?: string
  vertical?: boolean
}>()

const slots = defineSlots()

const hasDescription = computed(() => {
  return description || slots.description
})
</script>

<template>
  <Flex
    :align="vertical ? void 0 : 'center'"
    class="b-1 b-solid p-4 bg-elevated b-border-sec rounded-lg"
    :gap="vertical ? 'middle' : 'large'"
    justify="space-between"
    :vertical="vertical"
  >
    <Flex
      align="center"
      class="flex-1"
    >
      <Flex vertical>
        <div class="text-3.5 font-medium">
          {{ title }}
        </div>

        <div
          class="break-all text-3 color-text-tertiary [&_a]:color-text-tertiary [&_a]:active:color-blue-7 [&_a]:hover:color-blue-5"
          :class="{ 'mt-2': hasDescription }"
        >
          <slot name="description">
            {{ description }}
          </slot>
        </div>
      </Flex>
    </Flex>

    <slot />
  </Flex>
</template>
