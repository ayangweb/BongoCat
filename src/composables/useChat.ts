import { emit } from '@tauri-apps/api/event'

import { LISTEN_KEY } from '@/constants'

/**
 * 全局广播一条气泡。任意页面/composable 可调用。
 * 总开关 / 默认时长 / 定位 / 动画全部由 chat 页统一处理（见 src/pages/chat/index.vue）。
 * @param text 气泡文本
 * @param duration 毫秒；省略时由 chat 页用默认时长兜底；0 表示常驻。
 */
export function say(text: string, duration?: number) {
  return emit(LISTEN_KEY.SHOW_CHAT, { text, duration })
}
