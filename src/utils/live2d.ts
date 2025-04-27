import { Live2DModel } from 'pixi-live2d-display'
import { Application, Ticker } from 'pixi.js'

Live2DModel.registerTicker(Ticker)

class Live2d {
  private app: Application | null = null
  public model: Live2DModel | null = null

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

  public async load(url: string) {
    if (!this.app) {
      this.mount()
    }

    const model = await Live2DModel.from(url)

    if (this.app?.stage.children.length) {
      this.app.stage.removeChildren()
    }

    this.app?.stage.addChild(model)

    const { definitions, expressionManager } = model.internalModel.motionManager

    this.model = model

    return {
      motions: definitions,
      expressions: expressionManager?.definitions ?? [],
    }
  }

  public destroy() {
    this.model?.destroy()
  }

  public playMotion(group: string, index: number) {
    return this.model?.motion(group, index)
  }

  public playExpressions(index: number) {
    return this.model?.expression(index)
  }

  public setParameterValue(id: string, value: number | boolean) {
    return this.model?.internalModel.coreModel.setParameterValueById(id, Number(value))
  }
}

const live2d = new Live2d()

export default live2d
