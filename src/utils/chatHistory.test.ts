import assert from 'node:assert'

import type { ChatMessage } from './chatHistory'

import { appendCapped, filterHistory } from './chatHistory'

function msg(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return { time: 1000, text: 'hi', status: 'shown', source: 'internal', ...overrides }
}

// 1. appendCapped：未超限时直接追加
{
  const history: ChatMessage[] = []
  appendCapped(history, msg(), 3)
  appendCapped(history, msg({ time: 2000 }), 3)
  assert.strictEqual(history.length, 2)
  assert.strictEqual(history[1].time, 2000)
}

// 2. appendCapped：超限时移除最旧，保留最新
{
  const history: ChatMessage[] = []
  for (let i = 1; i <= 5; i++) {
    appendCapped(history, msg({ time: i }), 3)
  }
  assert.strictEqual(history.length, 3)
  assert.deepStrictEqual(history.map(m => m.time), [3, 4, 5])
}

// 3. filterHistory：空条件返回全部
{
  const history = [msg(), msg({ status: 'skipped' })]
  assert.strictEqual(filterHistory(history, {}).length, 2)
}

// 4. filterHistory：按状态过滤
{
  const history = [msg(), msg({ status: 'skipped' })]
  const out = filterHistory(history, { status: 'skipped' })
  assert.strictEqual(out.length, 1)
  assert.strictEqual(out[0].status, 'skipped')
}

// 5. filterHistory：按来源过滤
{
  const history = [msg(), msg({ source: 'http' })]
  const out = filterHistory(history, { source: 'http' })
  assert.strictEqual(out.length, 1)
  assert.strictEqual(out[0].source, 'http')
}

// 6. filterHistory：日期范围为闭区间
{
  const history = [msg({ time: 100 }), msg({ time: 200 }), msg({ time: 300 })]
  const out = filterHistory(history, { range: [100, 200] })
  assert.deepStrictEqual(out.map(m => m.time), [100, 200])
}

// 7. filterHistory：多条件同时生效
{
  const history = [
    msg({ time: 100, status: 'shown', source: 'http' }),
    msg({ time: 150, status: 'skipped', source: 'http' }),
    msg({ time: 200, status: 'skipped', source: 'internal' }),
  ]
  const out = filterHistory(history, { status: 'skipped', source: 'http', range: [100, 200] })
  assert.strictEqual(out.length, 1)
  assert.strictEqual(out[0].time, 150)
}

// eslint-disable-next-line no-console
console.log('chatHistory tests passed')
