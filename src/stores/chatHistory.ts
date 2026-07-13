import { defineStore } from 'pinia'
import { ref } from 'vue'

import type { ChatMessage } from '@/utils/chatHistory'

import { appendCapped } from '@/utils/chatHistory'

export const useChatHistoryStore = defineStore('chat-history', () => {
  const history = ref<ChatMessage[]>([])

  function record(message: ChatMessage) {
    appendCapped(history.value, message)
  }

  return {
    history,
    record,
  }
})
