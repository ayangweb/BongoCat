import assert from 'node:assert'

import { computeBubblePosition } from './chatPosition'

// 屏幕：原点在 (0,0)，1920x1080
const screen = { x: 0, y: 0, w: 1920, h: 1080 }
const gap = 10

// 1. 正常居中：猫在屏幕中央 → x 水平居中、y 在猫上方
{
  const main = { x: 860, y: 490, w: 200, h: 200 }
  const bubble = { w: 100, h: 60 }
  const { x, y } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 860 + (200 - 100) / 2) // 910
  assert.strictEqual(y, 490 - 60 - 10) // 420
}

// 2. 贴左边缘 → x 夹到 screen.x
{
  const main = { x: 0, y: 490, w: 200, h: 200 }
  const bubble = { w: 400, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 0)
}

// 3. 贴右边缘 → x 夹到 screen.x + screen.w - bubble.w
{
  const main = { x: 1820, y: 490, w: 100, h: 200 }
  const bubble = { w: 300, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 1920 - 300) // 1620
}

// 4. 贴顶部、上方放不下 → 翻到猫咪下方
{
  const main = { x: 860, y: 0, w: 200, h: 200 }
  const bubble = { w: 100, h: 60 }
  const { y } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(y, 0 + 200 + 10) // 210
}

// 5. 气泡比猫宽 → 仍以猫中心对齐
{
  const main = { x: 900, y: 490, w: 100, h: 200 }
  const bubble = { w: 300, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 900 + (100 - 300) / 2) // 800
}

// 6. 气泡比屏幕还宽 → 夹取后取 screen.x，不越界
{
  const main = { x: 860, y: 490, w: 200, h: 200 }
  const bubble = { w: 3000, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 0)
}

// 7. 多显示器：猫在副屏（负坐标偏移）→ 用副屏 bounds 夹取
{
  const sub = { x: -1920, y: 0, w: 1920, h: 1080 }
  const main = { x: -1920, y: 490, w: 200, h: 200 } // 贴副屏左边
  const bubble = { w: 400, h: 60 }
  const { x } = computeBubblePosition(main, bubble, sub, gap)
  assert.strictEqual(x, -1920)
}

// 8. DPI=2：调用方传入的已是物理像素，函数结果应为物理坐标
{
  const main = { x: 1720, y: 980, w: 400, h: 400 } // 物理（逻辑×2）
  const bubble = { w: 200, h: 120 } // 物理（逻辑 100x60 ×2）
  const hidpi = { x: 0, y: 0, w: 3840, h: 2160 }
  const { x, y } = computeBubblePosition(main, bubble, hidpi, 20)
  assert.strictEqual(x, 1720 + (400 - 200) / 2) // 1820
  assert.strictEqual(y, 980 - 120 - 20) // 840
}

// eslint-disable-next-line no-console
console.log('chatPosition: all assertions passed')
