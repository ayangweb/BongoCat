import assert from 'node:assert'

import { createSSEParser, decryptBark, resolveBarkText } from '../src/utils/bark'

async function main() {
  // --- SSE 解析：心跳注释行、事件跨 chunk、多行 data、\r\n 归一化 ---
  const feed = createSSEParser()

  assert.deepStrictEqual(feed(': ping\n\nevent: noti'), [], '心跳行应被忽略，半个事件应留在缓冲区')

  const events = feed('fication\nid: 1\ndata: {"a":\ndata: 1}\n\n')
  assert.strictEqual(events.length, 1)
  assert.strictEqual(events[0].event, 'notification')
  assert.strictEqual(events[0].id, '1')
  assert.strictEqual(events[0].data, '{"a":\n1}', '多行 data 用 \\n 拼接')

  const crlf = feed('event: ready\r\ndata: {}\r\n\r\n')
  assert.strictEqual(crlf.length, 1)
  assert.strictEqual(crlf[0].event, 'ready')

  // --- 解密：用 Node webcrypto 加密再解回（CBC / GCM），坏密文拒绝 ---
  const encoder = new TextEncoder()
  const keyStr = '0123456789abcdef' // 16 字符 = AES-128
  const ivCbc = 'abcdefghijklmnop' // CBC 16 字符
  const ivGcm = 'abcdefghijkl' // GCM 12 字符
  const plain = JSON.stringify({ title: 'hi', body: 'there' })
  const toB64 = (buf: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(buf)))

  const cbcKey = await crypto.subtle.importKey('raw', encoder.encode(keyStr), 'AES-CBC', false, ['encrypt'])
  const cbcB64 = toB64(await crypto.subtle.encrypt({ name: 'AES-CBC', iv: encoder.encode(ivCbc) }, cbcKey, encoder.encode(plain)))
  const cbcOut = await decryptBark(cbcB64, { mode: 'cbc', key: keyStr, iv: ivCbc })
  assert.strictEqual(cbcOut.title, 'hi')

  const gcmKey = await crypto.subtle.importKey('raw', encoder.encode(keyStr), 'AES-GCM', false, ['encrypt'])
  const gcmB64 = toB64(await crypto.subtle.encrypt({ name: 'AES-GCM', iv: encoder.encode(ivGcm) }, gcmKey, encoder.encode(plain)))
  const gcmOut = await decryptBark(gcmB64, { mode: 'gcm', key: keyStr, iv: ivGcm })
  assert.strictEqual(gcmOut.body, 'there')

  await assert.rejects(decryptBark('AAAAAAAA', { mode: 'gcm', key: keyStr, iv: ivGcm }), '坏密文必须抛错')

  // --- 文本映射：payload 优先、key 不区分大小写、title+body 换行拼接 ---
  assert.strictEqual(await resolveBarkText({ title: 'a', body: 'b' }), 'a\nb')
  assert.strictEqual(await resolveBarkText({ title: 'x', payload: { Title: 'a', Body: 'b' } }), 'a\nb', 'payload 覆盖顶层字段')
  assert.strictEqual(await resolveBarkText({ body: 'only' }), 'only', '无 title 时不出现多余换行')
  assert.strictEqual(await resolveBarkText({}), '', '空消息返回空串')

  // 加密 payload 端到端；payload 内 iv 覆盖本地配置
  const encText = await resolveBarkText({ payload: { ciphertext: cbcB64 } }, { mode: 'cbc', key: keyStr, iv: ivCbc })
  assert.strictEqual(encText, 'hi\nthere')
  const encIvOverride = await resolveBarkText({ payload: { ciphertext: cbcB64, iv: ivCbc } }, { mode: 'cbc', key: keyStr, iv: 'wrongwrongwrongw' })
  assert.strictEqual(encIvOverride, 'hi\nthere', 'payload.iv 应覆盖本地 iv')

  // 有 ciphertext 但未配密钥 → 抛错（上层丢弃该条）
  await assert.rejects(resolveBarkText({ payload: { ciphertext: cbcB64 } }))

  console.log('bark self-check OK')
}

main()
