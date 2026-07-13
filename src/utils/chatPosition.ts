export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

export interface Size {
  w: number
  h: number
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(value, max))
}

/**
 * 计算气泡左上角的物理像素坐标。
 * 所有入参均为物理像素：main=主窗口外框，bubble=气泡物理尺寸，
 * screen=猫所在显示器 bounds，gap=气泡与猫的间距。
 */
export function computeBubblePosition(main: Rect, bubble: Size, screen: Rect, gap: number) {
  let x = main.x + (main.w - bubble.w) / 2
  let y = main.y - bubble.h - gap

  x = clamp(x, screen.x, screen.x + screen.w - bubble.w)

  if (y < screen.y) {
    y = main.y + main.h + gap
  }

  return { x, y }
}
