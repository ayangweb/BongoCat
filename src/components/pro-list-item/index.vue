<script setup lang="ts">
import { Flex } from 'ant-design-vue'
import { computed, useSlots } from 'vue'

const { title, description, vertical } = defineProps<{
  title: string
  description?: string
  vertical?: boolean
}>()

const slots = useSlots()

const hasDescription = computed(() => {
  return description || slots.description
})
</script>

<template>
  <Flex
    :align="vertical ? void 0 : 'center'"
    class="b b-color-2 rounded-lg b-solid bg-white p-4 [&_.ant-slider]:m-0!"
    gap="middle"
    justify="space-between"
    :vertical="vertical"
  >
    <Flex align="center">
      <Flex vertical>
        <div class="text-sm font-medium">
          {{ title }}
        </div>

        <div
          class="break-all text-xs [&_a]:(active:text-color-primary-7 hover:text-color-primary-5 text-color-3) text-color-3"
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
