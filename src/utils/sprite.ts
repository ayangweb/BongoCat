import { convertFileSrc } from '@tauri-apps/api/core'
import { readTextFile } from '@tauri-apps/plugin-fs'

import { join } from './path'

export interface SpriteAnimationConfig {
  file: string
  frameWidth: number
  frameHeight: number
  frames: number
  columns: number
  fps: number
  loop: boolean
  frameDurations?: number[]
}

export type SpriteKeyboardBinding = string | string[]

export interface SpriteBindingsConfig {
  keyboard?: Record<string, SpriteKeyboardBinding>
  mouse?: Record<string, string>
}

export interface SpriteBubbleConfig {
  enabled: boolean
  duration: number
  rise: number
  fontSize: number
  maxVisible: number
  anchorX?: number
  anchorY?: number
  fillTop: string
  fill: string
  fillBottom: string
  highlightColor: string
  stroke: string
  strokeWidth: number
  textColor: string
  shadowColor: string
  shadowBlur: number
  shadowOffsetY: number
}

export interface SpriteModelConfig {
  renderer: 'sprite'
  id?: string
  displayName?: string
  mode?: 'standard' | 'keyboard' | 'gamepad'
  canvas: {
    width: number
    height: number
  }
  defaultAnimation: string
  animations: Record<string, SpriteAnimationConfig>
  bindings?: SpriteBindingsConfig
  keyboard?: Record<string, SpriteKeyboardBinding>
  mouse?: Record<string, string>
  bubbles?: Partial<SpriteBubbleConfig>
}

export interface SpriteModelSize {
  width: number
  height: number
}

export interface SpriteModelLoadResult {
  width: number
  height: number
  motions: Record<string, never[]>
  expressions: never[]
}

interface LoadedAnimation {
  config: SpriteAnimationConfig
  image: HTMLImageElement
}

interface ActiveBubble {
  text: string
  createdAt: number
  sequence: number
}

const defaultBubbleConfig: SpriteBubbleConfig = {
  enabled: true,
  duration: 900,
  rise: 48,
  fontSize: 22,
  maxVisible: 5,
  fillTop: 'rgba(255, 255, 255, 0.99)',
  fill: 'rgba(230, 250, 255, 0.98)',
  fillBottom: 'rgba(192, 235, 248, 0.97)',
  highlightColor: 'rgba(255, 255, 255, 0.92)',
  stroke: 'rgba(65, 174, 211, 0.9)',
  strokeWidth: 1.75,
  textColor: '#1f2f46',
  shadowColor: 'rgba(12, 30, 54, 0.42)',
  shadowBlur: 8,
  shadowOffsetY: 3,
}

class SpriteRenderer {
  private canvas: HTMLCanvasElement | null = null
  private context: CanvasRenderingContext2D | null = null
  private config: SpriteModelConfig | null = null
  private animations = new Map<string, LoadedAnimation>()
  private activeAnimation = ''
  private activeFrame = 0
  private animationFinished = false
  private frameStartedAt = 0
  private animationFrameId: number | null = null
  private loadGeneration = 0
  private maxFPS = 60
  private bindingIndexes = new Map<string, number>()
  private pressedKeyboard = new Map<string, string>()
  private pressedMouse = new Map<string, string>()
  private bubbles: ActiveBubble[] = []
  private bubbleConfig: SpriteBubbleConfig = { ...defaultBubbleConfig }
  private bubbleSequence = 0
  private lastRenderAt = Number.NEGATIVE_INFINITY
  private renderPending = false
  private mirrored = false

  public async load(path: string): Promise<SpriteModelLoadResult> {
    const generation = ++this.loadGeneration

    this.reset()

    const { animations, config } = await this.readAndValidateModel(path)

    if (generation !== this.loadGeneration) {
      throw new DOMException('Sprite model load was superseded', 'AbortError')
    }

    this.initCanvas()

    this.config = config
    this.animations = new Map(animations)
    this.bubbleConfig = { ...defaultBubbleConfig, ...config.bubbles }

    this.resizeModel(config.canvas)
    this.play(config.defaultAnimation)

    return {
      width: config.canvas.width,
      height: config.canvas.height,
      motions: {},
      expressions: [],
    }
  }

  public async validateModel(path: string) {
    const { config } = await this.readAndValidateModel(path)

    return config
  }

  public destroy() {
    ++this.loadGeneration
    this.reset()
  }

  public resizeModel(modelSize: SpriteModelSize) {
    if (!this.canvas || !this.context) return

    const width = Math.max(1, this.canvas.clientWidth || window.innerWidth || modelSize.width)
    const height = Math.max(1, this.canvas.clientHeight || window.innerHeight || modelSize.height)
    const density = Math.max(1, window.devicePixelRatio || 1)
    const pixelWidth = Math.round(width * density)
    const pixelHeight = Math.round(height * density)

    this.canvas.style.width = '100%'
    this.canvas.style.height = '100%'

    if (this.canvas.width !== pixelWidth || this.canvas.height !== pixelHeight) {
      this.canvas.width = pixelWidth
      this.canvas.height = pixelHeight
    }

    this.context.imageSmoothingEnabled = true
    this.context.imageSmoothingQuality = 'high'

    if (this.activeAnimation) {
      this.renderFrame()
    } else {
      this.context.clearRect(0, 0, this.canvas.width, this.canvas.height)
    }
  }

  public play(name: string) {
    const animation = this.animations.get(name)

    if (!animation) return false

    this.stopAnimationFrame()

    this.activeAnimation = name
    this.activeFrame = 0
    this.animationFinished = false
    const timestamp = performance.now()

    this.frameStartedAt = timestamp

    this.renderIfDue(timestamp)

    this.ensureAnimationFrame()

    return true
  }

  public handleKeyboard(key: string, pressed: boolean, label?: string) {
    const keyboard = this.config?.bindings?.keyboard ?? this.config?.keyboard

    if (!pressed) {
      if (!keyboard) return false

      const animationName = this.pressedKeyboard.get(key)

      this.pressedKeyboard.delete(key)

      return this.releaseBinding(animationName)
    }

    if (!keyboard) {
      return this.showBubble(key, label)
    }

    const bindingKey = this.resolveKeyboardBindingKey(keyboard, key)
    const binding = keyboard[bindingKey]

    if (!binding) {
      return this.showBubble(key, label)
    }

    const animationName = this.resolveKeyboardBinding(bindingKey, binding)

    this.pressedKeyboard.set(key, animationName)

    const animationPlayed = this.play(animationName)
    const bubbleShown = this.showBubble(key, label)

    return animationPlayed || bubbleShown
  }

  public handleMouse(button: string, pressed: boolean) {
    const mouse = this.config?.bindings?.mouse ?? this.config?.mouse

    if (!mouse) return false

    if (!pressed) {
      const animationName = this.pressedMouse.get(button)

      this.pressedMouse.delete(button)

      return this.releaseBinding(animationName)
    }

    const animationName = mouse[button] ?? mouse['*']

    if (!animationName) return false

    this.pressedMouse.set(button, animationName)

    return this.play(animationName)
  }

  public setMaxFPS(fps: number) {
    if (!Number.isFinite(fps)) return

    this.maxFPS = fps > 0 ? Math.max(1, fps) : Number.POSITIVE_INFINITY
  }

  public setMirrored(mirrored: boolean) {
    this.mirrored = mirrored

    this.renderFrame()
  }

  private readonly tick = (timestamp: number) => {
    this.animationFrameId = null

    const previousBubbleCount = this.bubbles.length

    this.bubbles = this.bubbles.filter((bubble) => {
      return timestamp - bubble.createdAt < this.bubbleConfig.duration
    })

    const animation = this.animations.get(this.activeAnimation)

    this.renderPending ||= previousBubbleCount !== this.bubbles.length

    let animationFinishedNow = false

    if (animation && !this.animationFinished) {
      let elapsed = timestamp - this.frameStartedAt
      let remainingAdvances = animation.config.frames * 2

      while (remainingAdvances > 0) {
        const frameDuration = this.getFrameDuration(animation.config, this.activeFrame)

        if (elapsed < frameDuration) break

        this.frameStartedAt += frameDuration
        elapsed -= frameDuration
        remainingAdvances--

        if (this.activeFrame + 1 < animation.config.frames) {
          this.activeFrame++
          this.renderPending = true

          continue
        }

        if (animation.config.loop) {
          this.activeFrame = 0
          this.renderPending = true

          continue
        }

        this.animationFinished = true
        animationFinishedNow = true

        if (this.activeAnimation !== this.config?.defaultAnimation) {
          this.playDefault()

          return
        }

        this.renderPending = true

        break
      }

      if (remainingAdvances === 0) {
        this.frameStartedAt = timestamp
      }
    }

    const renderInterval = 1000 / this.maxFPS
    const bubbleExpired = previousBubbleCount > 0 && this.bubbles.length === 0

    if (this.bubbles.length > 0) {
      this.renderPending = true
    }

    if (bubbleExpired || animationFinishedNow
      || (this.renderPending && timestamp - this.lastRenderAt >= renderInterval)) {
      this.renderFrame(timestamp)
    }

    this.ensureAnimationFrame()
  }

  private initCanvas() {
    const canvas = document.getElementById('spriteCanvas')

    if (!(canvas instanceof HTMLCanvasElement)) {
      throw new TypeError('Canvas #spriteCanvas was not found')
    }

    const context = canvas.getContext('2d', { alpha: true })

    if (!context) {
      throw new Error('Canvas 2D is unavailable')
    }

    context.imageSmoothingEnabled = true
    context.imageSmoothingQuality = 'high'

    this.canvas = canvas
    this.context = context
  }

  private renderFrame(timestamp = performance.now()) {
    if (!this.canvas || !this.context || !this.config) return

    const animation = this.animations.get(this.activeAnimation)

    this.context.clearRect(0, 0, this.canvas.width, this.canvas.height)

    if (animation) {
      const { frameWidth, frameHeight, columns } = animation.config
      const column = this.activeFrame % columns
      const row = Math.floor(this.activeFrame / columns)
      const canvasWidth = this.config.canvas.width
      const canvasHeight = this.config.canvas.height
      const frameScale = Math.min(canvasWidth / frameWidth, canvasHeight / frameHeight)
      const viewportScale = Math.min(
        this.canvas.width / canvasWidth,
        this.canvas.height / canvasHeight,
      )
      const targetWidth = frameWidth * frameScale * viewportScale
      const targetHeight = frameHeight * frameScale * viewportScale
      const targetX = (this.canvas.width - targetWidth) / 2
      const targetY = (this.canvas.height - targetHeight) / 2

      this.context.save()

      if (this.mirrored) {
        this.context.translate(this.canvas.width, 0)
        this.context.scale(-1, 1)
      }

      this.context.drawImage(
        animation.image,
        column * frameWidth,
        row * frameHeight,
        frameWidth,
        frameHeight,
        targetX,
        targetY,
        targetWidth,
        targetHeight,
      )

      this.context.restore()
    }

    this.renderBubbles(timestamp)

    this.lastRenderAt = timestamp
    this.renderPending = false
  }

  private renderBubbles(timestamp: number) {
    if (!this.canvas || !this.context || !this.config || !this.bubbleConfig.enabled) return

    const viewportScale = Math.min(
      this.canvas.width / this.config.canvas.width,
      this.canvas.height / this.config.canvas.height,
    )
    const rise = this.bubbleConfig.rise * viewportScale
    const modelOffsetX = (this.canvas.width - this.config.canvas.width * viewportScale) / 2
    const modelOffsetY = (this.canvas.height - this.config.canvas.height * viewportScale) / 2
    const anchorX = modelOffsetX
      + (this.bubbleConfig.anchorX ?? this.config.canvas.width / 2) * viewportScale
    const anchorY = modelOffsetY
      + (this.bubbleConfig.anchorY ?? this.config.canvas.height * 0.2) * viewportScale
    const slotSpan = Math.min(
      this.bubbleConfig.fontSize * viewportScale * 3.6,
      this.canvas.width / (this.bubbleConfig.maxVisible + 1),
    )

    for (const bubble of this.bubbles) {
      const progress = (timestamp - bubble.createdAt) / this.bubbleConfig.duration

      if (progress < 0 || progress >= 1) continue

      const slot = bubble.sequence % this.bubbleConfig.maxVisible
      const slotDirection = slot % 2 === 0 ? 1 : -1
      const slotDistance = Math.floor(slot / 2) + 0.62
      const slotOffset = slotDistance * slotDirection
      const fontSize = this.fitBubbleFontSize(bubble.text, viewportScale)
      const paddingX = fontSize * 0.78
      const cloudHeight = fontSize * 1.82
      const tailHeight = fontSize * 0.34

      this.context.save()
      this.context.font = `700 ${fontSize}px ui-rounded, "SF Pro Rounded", system-ui, sans-serif`

      const textWidth = this.context.measureText(bubble.text).width
      const cloudWidth = Math.max(cloudHeight * 1.08, textWidth + paddingX * 2)
      const halfWidth = cloudWidth / 2
      const margin = Math.max(
        3,
        (this.bubbleConfig.shadowBlur + Math.abs(this.bubbleConfig.shadowOffsetY)
          + this.bubbleConfig.strokeWidth + 2) * viewportScale,
      )
      const enterProgress = Math.min(1, progress / 0.18)
      const exitProgress = Math.max(0, Math.min(1, (progress - 0.72) / 0.28))
      const separationProgress = this.smoothstep(Math.min(1, progress / 0.14))
      const motionProgress = this.smoothstep(Math.min(1, progress / 0.12))
      const riseProgress = 1 - (1 - progress) ** 1.22
      const phase = progress * Math.PI * 3.2 + bubble.sequence * 1.31
      const softSway = Math.sin(phase) * fontSize * 0.1 * motionProgress
      const unclampedTipX = anchorX
        + slotOffset * slotSpan * separationProgress
        + softSway
      const tailTipX = Math.max(
        halfWidth + margin,
        Math.min(this.canvas.width - halfWidth - margin, unclampedTipX),
      )
      const tailTipY = anchorY
        + Math.abs(slotOffset) * fontSize * 0.18 * separationProgress
        - riseProgress * rise
        + Math.sin(phase * 0.72) * fontSize * 0.035 * motionProgress
      const enterScale = 0.62 + 0.38 * this.easeOutBack(enterProgress)
      const squashStretch = Math.sin(enterProgress * Math.PI) * (1 - enterProgress * 0.35)
      const breathing = Math.sin(phase * 0.82)
      const exitScale = 1 - this.smoothstep(exitProgress) * 0.09
      const scaleX = enterScale * (1 - squashStretch * 0.11 + breathing * 0.018) * exitScale
      const scaleY = enterScale * (1 + squashStretch * 0.14 - breathing * 0.022) * exitScale
      const rotation = Math.sin(phase * 0.6) * 0.018 * motionProgress
      const opacity = (0.86 + this.smoothstep(enterProgress) * 0.14)
        * (1 - this.smoothstep(exitProgress))
      const asymmetry = ((bubble.sequence * 37) % 7 - 3) / 3

      this.context.translate(tailTipX, tailTipY)
      this.context.rotate(rotation)
      this.context.scale(scaleX, scaleY)
      this.traceCloudBubblePath(cloudWidth, cloudHeight, tailHeight, asymmetry)

      const fill = this.context.createLinearGradient(
        0,
        -tailHeight - cloudHeight,
        0,
        -tailHeight,
      )

      fill.addColorStop(0, this.bubbleConfig.fillTop)
      fill.addColorStop(0.52, this.bubbleConfig.fill)
      fill.addColorStop(1, this.bubbleConfig.fillBottom)

      this.context.globalAlpha = opacity
      this.context.fillStyle = fill
      this.context.strokeStyle = this.bubbleConfig.stroke
      this.context.lineWidth = Math.max(1, viewportScale * this.bubbleConfig.strokeWidth)
      this.context.lineJoin = 'round'
      this.context.shadowColor = this.bubbleConfig.shadowColor
      this.context.shadowBlur = this.bubbleConfig.shadowBlur * viewportScale
      this.context.shadowOffsetY = this.bubbleConfig.shadowOffsetY * viewportScale
      this.context.fill()
      this.context.stroke()

      this.context.shadowColor = 'transparent'
      this.context.shadowBlur = 0
      this.context.shadowOffsetY = 0

      this.context.save()
      this.context.clip()
      this.traceCloudHighlight(cloudWidth, cloudHeight, tailHeight, asymmetry)
      this.context.strokeStyle = this.bubbleConfig.highlightColor
      this.context.lineWidth = Math.max(1.2, fontSize * 0.065)
      this.context.lineCap = 'round'
      this.context.globalAlpha = opacity * 0.86
      this.context.stroke()

      const glint = this.context.createRadialGradient(
        -cloudWidth * 0.23,
        -tailHeight - cloudHeight * 0.7,
        0,
        -cloudWidth * 0.23,
        -tailHeight - cloudHeight * 0.7,
        fontSize * 0.16,
      )

      glint.addColorStop(0, this.bubbleConfig.highlightColor)
      glint.addColorStop(1, 'rgba(255, 255, 255, 0)')
      this.context.fillStyle = glint
      this.context.beginPath()
      this.context.arc(
        -cloudWidth * 0.23,
        -tailHeight - cloudHeight * 0.7,
        fontSize * 0.16,
        0,
        Math.PI * 2,
      )
      this.context.fill()
      this.context.restore()

      this.context.fillStyle = this.bubbleConfig.textColor
      this.context.textAlign = 'center'
      this.context.textBaseline = 'middle'
      this.context.lineWidth = Math.max(1.5, fontSize * 0.09)
      this.context.strokeStyle = 'rgba(255, 255, 255, 0.74)'
      this.context.strokeText(bubble.text, 0, -tailHeight - cloudHeight * 0.47)
      this.context.fillText(bubble.text, 0, -tailHeight - cloudHeight * 0.47)
      this.context.restore()
    }
  }

  private traceCloudBubblePath(
    width: number,
    height: number,
    tailHeight: number,
    asymmetry: number,
  ) {
    if (!this.context) return

    const left = -width / 2
    const right = width / 2
    const top = -tailHeight - height
    const bottom = -tailHeight
    const tailWidth = tailHeight * 1.18
    const crestShift = asymmetry * width * 0.035

    this.context.beginPath()
    this.context.moveTo(-tailWidth, bottom)
    this.context.bezierCurveTo(
      -width * 0.2,
      bottom + height * 0.035,
      left + width * 0.18,
      bottom + height * 0.02,
      left + width * 0.1,
      bottom - height * 0.17,
    )
    this.context.bezierCurveTo(
      left - width * 0.018,
      bottom - height * 0.24,
      left - width * 0.018,
      bottom - height * 0.42,
      left + width * 0.09,
      bottom - height * 0.5,
    )
    this.context.bezierCurveTo(
      left + width * 0.02,
      top + height * 0.27,
      left + width * 0.12,
      top + height * 0.13,
      left + width * 0.27,
      top + height * 0.2,
    )
    this.context.bezierCurveTo(
      left + width * 0.29,
      top + height * 0.045,
      left + width * 0.43,
      top - height * 0.035,
      left + width * 0.55 + crestShift,
      top + height * 0.07,
    )
    this.context.bezierCurveTo(
      left + width * 0.67,
      top - height * 0.005,
      left + width * 0.77,
      top + height * 0.055,
      right - width * 0.18,
      top + height * 0.19,
    )
    this.context.bezierCurveTo(
      right - width * 0.035,
      top + height * 0.16,
      right + width * 0.016,
      top + height * 0.35,
      right - width * 0.03,
      bottom - height * 0.49,
    )
    this.context.bezierCurveTo(
      right + width * 0.015,
      bottom - height * 0.3,
      right - width * 0.07,
      bottom - height * 0.11,
      right - width * 0.2,
      bottom - height * 0.13,
    )
    this.context.bezierCurveTo(
      right - width * 0.27,
      bottom + height * 0.025,
      width * 0.2,
      bottom + height * 0.035,
      tailWidth,
      bottom,
    )
    this.context.bezierCurveTo(
      tailWidth * 0.8,
      bottom + tailHeight * 0.48,
      tailWidth * 0.38,
      -tailHeight * 0.06,
      0,
      0,
    )
    this.context.bezierCurveTo(
      -tailWidth * 0.38,
      -tailHeight * 0.06,
      -tailWidth * 0.8,
      bottom + tailHeight * 0.48,
      -tailWidth,
      bottom,
    )
    this.context.closePath()
  }

  private traceCloudHighlight(
    width: number,
    height: number,
    tailHeight: number,
    asymmetry: number,
  ) {
    if (!this.context) return

    const top = -tailHeight - height

    this.context.beginPath()
    this.context.moveTo(-width * 0.32, top + height * 0.34)
    this.context.bezierCurveTo(
      -width * 0.23,
      top + height * 0.11,
      -width * 0.09 + asymmetry * width * 0.015,
      top + height * 0.08,
      width * 0.03,
      top + height * 0.16,
    )
    this.context.bezierCurveTo(
      width * 0.14,
      top + height * 0.08,
      width * 0.25,
      top + height * 0.16,
      width * 0.31,
      top + height * 0.28,
    )
  }

  private smoothstep(value: number) {
    return value * value * (3 - 2 * value)
  }

  private easeOutBack(value: number) {
    const overshoot = 1.70158
    const shifted = value - 1

    return 1 + (overshoot + 1) * shifted ** 3 + overshoot * shifted ** 2
  }

  private fitBubbleFontSize(text: string, viewportScale: number) {
    if (!this.context || !this.canvas) return this.bubbleConfig.fontSize * viewportScale

    const fontSize = this.bubbleConfig.fontSize * viewportScale

    this.context.font = `700 ${fontSize}px ui-rounded, "SF Pro Rounded", system-ui, sans-serif`

    const textWidth = this.context.measureText(text).width
    const maxTextWidth = this.canvas.width * 0.8 - fontSize * 1.3

    if (textWidth <= maxTextWidth) return fontSize

    return Math.max(fontSize * 0.55, fontSize * (maxTextWidth / textWidth))
  }

  private showBubble(key: string, label?: string) {
    if (!this.canvas || !this.context || !this.config || !this.bubbleConfig.enabled) return false

    const timestamp = performance.now()
    const actualLabel = label?.trim()
    const bubbleLabel = actualLabel
      && this.isDisplayableLabel(actualLabel)
      ? actualLabel
      : this.formatKeyLabel(key)

    this.bubbles.push({
      text: bubbleLabel,
      createdAt: timestamp,
      sequence: this.bubbleSequence++,
    })

    if (this.bubbles.length > this.bubbleConfig.maxVisible) {
      this.bubbles.splice(0, this.bubbles.length - this.bubbleConfig.maxVisible)
    }

    this.renderPending = true
    this.renderFrame(timestamp)
    this.ensureAnimationFrame()

    return true
  }

  private formatKeyLabel(key: string) {
    const letter = /^Key([A-Z])$/.exec(key)

    if (letter) return letter[1]

    const number = /^(?:Num|Digit)(\d)$/.exec(key)

    if (number) return number[1]

    const numpadNumber = /^Kp(\d)$/.exec(key)

    if (numpadNumber) return `Num ${numpadNumber[1]}`

    if (key === 'Return' || key === 'Enter') return 'Enter'
    if (key === 'KpReturn') return 'Num Enter'
    if (key === 'Space') return 'Space'
    if (key === 'BackQuote' || key === 'Backquote') return '`'

    const symbols: Record<string, string> = {
      Alt: 'Alt',
      AltGr: 'AltGr',
      Backspace: 'Backspace',
      CapsLock: 'Caps Lock',
      ControlLeft: 'Left Ctrl',
      ControlRight: 'Right Ctrl',
      Delete: 'Delete',
      End: 'End',
      Escape: 'Esc',
      Home: 'Home',
      ShiftLeft: 'Left Shift',
      ShiftRight: 'Right Shift',
      MetaLeft: 'Left Meta',
      MetaRight: 'Right Meta',
      PageDown: 'Page Down',
      PageUp: 'Page Up',
      Tab: 'Tab',
      LeftArrow: '←',
      RightArrow: '→',
      UpArrow: '↑',
      DownArrow: '↓',
      Function: 'Fn',
      KpMinus: 'Num -',
      KpPlus: 'Num +',
      KpMultiply: 'Num ×',
      KpDivide: 'Num ÷',
      KpDecimal: 'Num .',
      KpEqual: 'Num =',
      KpComma: 'Num ,',
      Minus: '-',
      Equal: '=',
      Comma: ',',
      Dot: '.',
      Slash: '/',
      SemiColon: ';',
      Quote: '\'',
      LeftBracket: '[',
      RightBracket: ']',
      BackSlash: '\\',
    }

    return symbols[key] ?? key
  }

  private resolveKeyboardBindingKey(
    keyboard: Record<string, SpriteKeyboardBinding>,
    key: string,
  ) {
    const candidates = key === 'Return'
      ? [key, 'Enter']
      : key === 'Enter'
        ? [key, 'Return']
        : [key]

    return candidates.find((candidate) => {
      return Object.prototype.hasOwnProperty.call(keyboard, candidate)
    }) ?? '*'
  }

  private resolveKeyboardBinding(bindingKey: string, binding: SpriteKeyboardBinding) {
    if (typeof binding === 'string') return binding

    const index = this.bindingIndexes.get(bindingKey) ?? 0

    this.bindingIndexes.set(bindingKey, index + 1)

    return binding[index % binding.length]
  }

  private releaseBinding(animationName?: string) {
    if (!animationName || animationName !== this.activeAnimation) return false

    const animation = this.animations.get(animationName)

    if (!animation?.config.loop) return false

    return this.playDefault()
  }

  private playDefault() {
    const defaultAnimation = this.config?.defaultAnimation

    if (!defaultAnimation) return false

    return this.play(defaultAnimation)
  }

  private ensureAnimationFrame() {
    if (this.animationFrameId !== null || !this.needsAnimationFrame()) return

    this.animationFrameId = requestAnimationFrame(this.tick)
  }

  private needsAnimationFrame() {
    if (this.renderPending) return true
    if (this.bubbles.length > 0) return true

    const animation = this.animations.get(this.activeAnimation)

    if (!animation || this.animationFinished) return false

    return animation.config.frames > 1 || !animation.config.loop
  }

  private stopAnimationFrame() {
    if (this.animationFrameId === null) return

    cancelAnimationFrame(this.animationFrameId)

    this.animationFrameId = null
  }

  private reset() {
    this.stopAnimationFrame()

    this.context?.clearRect(0, 0, this.canvas?.width ?? 0, this.canvas?.height ?? 0)

    this.canvas = null
    this.context = null
    this.config = null
    this.animations.clear()
    this.activeAnimation = ''
    this.activeFrame = 0
    this.animationFinished = false
    this.frameStartedAt = 0
    this.bindingIndexes.clear()
    this.pressedKeyboard.clear()
    this.pressedMouse.clear()
    this.bubbles = []
    this.bubbleConfig = { ...defaultBubbleConfig }
    this.bubbleSequence = 0
    this.lastRenderAt = Number.NEGATIVE_INFINITY
    this.renderPending = false
  }

  private async readAndValidateModel(path: string) {
    const configPath = join(path, 'model.json')
    const config = JSON.parse(await readTextFile(configPath)) as unknown

    this.assertConfig(config)

    const animations = await Promise.all(
      Object.entries(config.animations).map(async ([name, animation]) => {
        const image = await this.loadImage(convertFileSrc(join(path, animation.file)))

        this.assertSpritesheet(name, animation, image)

        return [name, { config: animation, image }] as const
      }),
    )

    return { animations, config }
  }

  private loadImage(source: string) {
    return new Promise<HTMLImageElement>((resolve, reject) => {
      const image = new Image()

      image.onload = () => resolve(image)
      image.onerror = () => reject(new Error(`Failed to load sprite image: ${source}`))
      image.src = source
    })
  }

  private assertConfig(config: unknown): asserts config is SpriteModelConfig {
    if (!config || typeof config !== 'object') {
      throw new Error('Invalid sprite model config')
    }

    const candidate = config as Partial<SpriteModelConfig>

    if (candidate.renderer !== 'sprite') {
      throw new Error('Sprite model renderer must be "sprite"')
    }

    for (const [name, value] of Object.entries({
      id: candidate.id,
      displayName: candidate.displayName,
    })) {
      if (value !== undefined && (typeof value !== 'string' || value.trim().length === 0)) {
        throw new TypeError(`Sprite model ${name} is invalid`)
      }
    }

    if (candidate.mode !== undefined
      && !['standard', 'keyboard', 'gamepad'].includes(candidate.mode)) {
      throw new TypeError('Sprite model mode is invalid')
    }

    if (!candidate.canvas || !this.isPositiveNumber(candidate.canvas.width)
      || !this.isPositiveNumber(candidate.canvas.height)) {
      throw new Error('Sprite model canvas dimensions are invalid')
    }

    if (!candidate.animations || typeof candidate.animations !== 'object'
      || Array.isArray(candidate.animations)
      || Object.keys(candidate.animations).length === 0) {
      throw new Error('Sprite model animations are missing')
    }

    if (typeof candidate.defaultAnimation !== 'string'
      || !candidate.animations[candidate.defaultAnimation]) {
      throw new Error('Sprite model default animation is invalid')
    }

    for (const [name, animation] of Object.entries(candidate.animations)) {
      if (!animation || !this.isRelativeAssetPath(animation.file)
        || !this.isPositiveInteger(animation.frameWidth)
        || !this.isPositiveInteger(animation.frameHeight)
        || !this.isPositiveInteger(animation.frames)
        || !this.isPositiveInteger(animation.columns)
        || !this.isPositiveNumber(animation.fps)
        || typeof animation.loop !== 'boolean') {
        throw new Error(`Sprite animation "${name}" is invalid`)
      }

      if (animation.frameDurations !== undefined
        && (!Array.isArray(animation.frameDurations)
          || animation.frameDurations.length !== animation.frames
          || animation.frameDurations.some(duration => !this.isPositiveNumber(duration)))) {
        throw new Error(`Sprite animation "${name}" frame durations are invalid`)
      }
    }

    if (candidate.bindings !== undefined
      && (!candidate.bindings || typeof candidate.bindings !== 'object'
        || Array.isArray(candidate.bindings))) {
      throw new TypeError('Sprite model bindings are invalid')
    }

    const keyboard = candidate.bindings?.keyboard ?? candidate.keyboard

    if (keyboard !== undefined
      && (!keyboard || typeof keyboard !== 'object' || Array.isArray(keyboard))) {
      throw new TypeError('Sprite keyboard bindings are invalid')
    }

    for (const [key, binding] of Object.entries(keyboard ?? {})) {
      const animationNames = typeof binding === 'string' ? [binding] : binding

      if (!Array.isArray(animationNames) || animationNames.length === 0
        || animationNames.some(item => typeof item !== 'string')) {
        throw new Error(`Sprite keyboard binding "${key}" is invalid`)
      }

      if (animationNames.some(name => !candidate.animations?.[name])) {
        throw new Error(`Sprite keyboard binding "${key}" references a missing animation`)
      }
    }

    const mouse = candidate.bindings?.mouse ?? candidate.mouse

    if (mouse !== undefined
      && (!mouse || typeof mouse !== 'object' || Array.isArray(mouse))) {
      throw new TypeError('Sprite mouse bindings are invalid')
    }

    for (const [button, binding] of Object.entries(mouse ?? {})) {
      if (typeof binding !== 'string') {
        throw new TypeError(`Sprite mouse binding "${button}" is invalid`)
      }

      if (!candidate.animations[binding]) {
        throw new Error(`Sprite mouse binding "${button}" references a missing animation`)
      }
    }

    if (candidate.bubbles !== undefined) {
      if (!candidate.bubbles || typeof candidate.bubbles !== 'object'
        || Array.isArray(candidate.bubbles)) {
        throw new TypeError('Sprite bubble config is invalid')
      }

      const bubbles = candidate.bubbles

      if (bubbles.enabled !== undefined && typeof bubbles.enabled !== 'boolean') {
        throw new TypeError('Sprite bubble enabled value is invalid')
      }

      for (const [name, value] of Object.entries({
        duration: bubbles.duration,
        rise: bubbles.rise,
        fontSize: bubbles.fontSize,
        strokeWidth: bubbles.strokeWidth,
      })) {
        if (value !== undefined && !this.isPositiveNumber(value)) {
          throw new TypeError(`Sprite bubble ${name} is invalid`)
        }
      }

      for (const [name, value] of Object.entries({
        anchorX: bubbles.anchorX,
        anchorY: bubbles.anchorY,
        shadowBlur: bubbles.shadowBlur,
        shadowOffsetY: bubbles.shadowOffsetY,
      })) {
        if (value !== undefined && !this.isNonNegativeNumber(value)) {
          throw new TypeError(`Sprite bubble ${name} is invalid`)
        }
      }

      if (bubbles.anchorX !== undefined && bubbles.anchorX > candidate.canvas.width) {
        throw new TypeError('Sprite bubble anchorX exceeds the model canvas')
      }

      if (bubbles.anchorY !== undefined && bubbles.anchorY > candidate.canvas.height) {
        throw new TypeError('Sprite bubble anchorY exceeds the model canvas')
      }

      if (bubbles.maxVisible !== undefined && !this.isPositiveInteger(bubbles.maxVisible)) {
        throw new TypeError('Sprite bubble maxVisible is invalid')
      }

      for (const [name, value] of Object.entries({
        fill: bubbles.fill,
        fillTop: bubbles.fillTop,
        fillBottom: bubbles.fillBottom,
        highlightColor: bubbles.highlightColor,
        stroke: bubbles.stroke,
        textColor: bubbles.textColor,
        shadowColor: bubbles.shadowColor,
      })) {
        if (value !== undefined && (typeof value !== 'string' || value.length === 0)) {
          throw new TypeError(`Sprite bubble ${name} is invalid`)
        }
      }
    }
  }

  private assertSpritesheet(
    name: string,
    animation: SpriteAnimationConfig,
    image: HTMLImageElement,
  ) {
    const requiredColumns = Math.min(animation.frames, animation.columns)
    const requiredRows = Math.ceil(animation.frames / animation.columns)

    if (image.naturalWidth < requiredColumns * animation.frameWidth
      || image.naturalHeight < requiredRows * animation.frameHeight) {
      throw new Error(`Sprite animation "${name}" exceeds its spritesheet bounds`)
    }
  }

  private isPositiveInteger(value: unknown): value is number {
    return Number.isInteger(value) && Number(value) > 0
  }

  private isPositiveNumber(value: unknown): value is number {
    return typeof value === 'number' && Number.isFinite(value) && value > 0
  }

  private isNonNegativeNumber(value: unknown): value is number {
    return typeof value === 'number' && Number.isFinite(value) && value >= 0
  }

  private isDisplayableLabel(value: string) {
    return Array.from(value).every((character) => {
      const codePoint = character.codePointAt(0) ?? 0

      return codePoint >= 0x20
        && !(codePoint >= 0x7F && codePoint <= 0x9F)
        && !(codePoint >= 0xE000 && codePoint <= 0xF8FF)
        && !(codePoint >= 0xF0000 && codePoint <= 0xFFFFD)
        && !(codePoint >= 0x100000 && codePoint <= 0x10FFFD)
    })
  }

  private isRelativeAssetPath(value: unknown): value is string {
    if (typeof value !== 'string' || value.trim().length === 0) return false
    if (/^(?:[\\/]|[a-z][a-z\d+.-]*:)/i.test(value)) return false

    return !value.split(/[\\/]/).includes('..')
  }

  private getFrameDuration(animation: SpriteAnimationConfig, frame: number) {
    return animation.frameDurations?.[frame] ?? 1000 / animation.fps
  }

  private renderIfDue(timestamp = performance.now()) {
    if (timestamp - this.lastRenderAt < 1000 / this.maxFPS) {
      this.renderPending = true

      return false
    }

    this.renderFrame(timestamp)

    return true
  }
}

export const sprite = new SpriteRenderer()

export default sprite
