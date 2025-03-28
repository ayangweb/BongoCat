<script setup lang="ts">
import { onMounted } from 'vue'

import { useTray } from '../composables/useTray'

const { createTray } = useTray()

onMounted(() => {
  createTray()
})
</script>

<template>
  <div class="">
    Preference
  </div>
</template>
