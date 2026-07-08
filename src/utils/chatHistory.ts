export const MAX_HISTORY = 500

export interface ChatMessage {
  time: number // 毫秒时间戳，展示时格式化到秒
  text: string
  status: 'shown' | 'skipped' // skipped = 总开关关闭时收到
  source: 'http' | 'internal'
}

export interface HistoryFilter {
  status?: ChatMessage['status']
  source?: ChatMessage['source']
  range?: [number, number] // 闭区间，毫秒时间戳
}

// 原地追加并裁剪到上限（移除最旧）
export function appendCapped(history: ChatMessage[], message: ChatMessage, limit = MAX_HISTORY) {
  history.push(message)

  if (history.length > limit) {
    history.splice(0, history.length - limit)
  }
}

export function filterHistory(history: ChatMessage[], filter: HistoryFilter): ChatMessage[] {
  return history.filter(({ time, status, source }) => {
    if (filter.status && status !== filter.status) return false
    if (filter.source && source !== filter.source) return false
    if (filter.range && (time < filter.range[0] || time > filter.range[1])) return false
    return true
  })
}
