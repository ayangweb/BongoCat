<script setup lang="ts">
import { useRemoteStore } from '@/stores/remote'

const remoteStore = useRemoteStore()
</script>

<template>
  <div class="remote-settings">
    <a-input v-model:value="remoteStore.serverIp" placeholder="Enter server IP" />
    <a-button @click="remoteStore.connect">Connect</a-button>
    <a-button @click="remoteStore.disconnect">Disconnect</a-button>
  </div>
</template>
