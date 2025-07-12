import { Channel, invoke } from '@tauri-apps/api/core'
import { watch } from 'vue'

import { INVOKE_KEY } from '@/constants'
import { useModelStore } from '@/stores/model'

export function useGamepad() {
  const { currentModel } = useModelStore()

  const startListening = () => {
    const channel = new Channel()

    channel.onmessage = (response) => {
      console.log('response', response)
    }

    invoke(INVOKE_KEY.START_GAMEPAD_LISTING, { channel })
  }

  watch(() => currentModel?.mode, (value) => {
    if (value === 'gamepad') {
      return startListening()
    }

    invoke(INVOKE_KEY.STOP_GAMEPAD_LISTING)
  }, { immediate: true })
}
