<script setup lang="ts">
import { Flex } from 'ant-design-vue'
import { onMounted } from 'vue'

import UpdateApp from '@/components/update-app/index.vue'
import { useTray } from '@/composables/useTray'
import { preferenceRoutes } from '@/router'
import { isMac } from '@/utils/platform'

const { createTray } = useTray()

onMounted(async () => {
  createTray()
})
</script>

<template>
  <Flex class="h-screen">
    <div class="h-full w-40 bg-color-8" :class="[isMac ? 'pt-8' : 'pt-4']" data-tauri-drag-region>
      <Flex class="px-2" gap="small" vertical>
        <RouterLink v-for="item in preferenceRoutes" :key="item.path" active-class="bg-primary! text-white! font-bold" class="h-10 flex items-center gap-2 rounded-lg hover:bg-color-6 px-4 text-color-1! transition" :to="item.path" @click.stop>
          <div class="size-5" :class="item.meta?.icon" />

          <span>{{ item.meta?.title }}</span>
        </RouterLink>
      </Flex>
    </div>

    <div class="flex-1 p-4" data-tauri-drag-region>
      <RouterView />
    </div>
  </Flex>

  <UpdateApp />
</template>
