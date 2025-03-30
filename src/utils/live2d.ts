import type { ModelType } from '../types/model'

import { Live2DModel } from 'pixi-live2d-display'
import { Application, Ticker } from 'pixi.js'

import { MODEL_PATH } from '../constants'

Live2DModel.registerTicker(Ticker)

interface Motion {
  Name: string
  File: string
  Sound?: string
  FadeInTime: number
  FadeOutTime: number
}

interface Expression {
  Name: string
  File: string
}

class ModelManager {
  private app: Application | null = null
  public currentModel: Live2DModel | null = null
  public currentMotions = new Map<string, Motion[]>()
  public currentExpressions = new Map<string, Expression>()

  constructor() { }

  private mount() {
    const view = document.getElementById('live2dCanvas') as HTMLCanvasElement

    this.app = new Application({
      view,
      resizeTo: window,
      backgroundAlpha: 0,
      autoDensity: true,
      resolution: devicePixelRatio,
    })
  }

  public async load(type: ModelType) {
    const modelPath = MODEL_PATH[type]

    if (!this.app) {
      this.mount()
    }

    const model = await Live2DModel.from(modelPath)

    if (this.app?.stage.children.length) {
      this.app.stage.removeChildren()
    }

    this.app?.stage.addChild(model)

    const { definitions, expressionManager } = model.internalModel.motionManager

    this.currentModel = model
    this.currentMotions = new Map(
      Object.entries(definitions),
    ) as Map<string, Motion[]>
    this.currentExpressions = new Map(
      Object.entries(expressionManager?.definitions || {}),
    ) as Map<string, Expression>
  }

  public destroy() {
    this.currentModel?.destroy()
  }

  public setParameterValue(id: string, value: number | boolean) {
    return this.currentModel?.internalModel.coreModel.setParameterValueById(id, Number(value))
  }
}

const live2d = new ModelManager()

export default live2d
