import { Live2DModel } from 'pixi-live2d-display'
import { Application, Ticker } from 'pixi.js'

Live2DModel.registerTicker(Ticker)

class Live2d {
  private app: Application | null = null
  public model: Live2DModel | null = null

  constructor() { }

  private create() {
    this.app = new Application({
      resizeTo: window,
      backgroundAlpha: 0,
      autoDensity: true,
      resolution: devicePixelRatio,
    })
  }

  public mount(anchor: HTMLElement) {
    if (!this.app && import.meta.env.DEV) {
      console.warn('Perform the mount operation after creating the PixiJS Application instance.')
    }
    this.app && anchor.appendChild(this.app.view)
  }

  public async load(url: string) {
    if (!this.app) {
      this.create()
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
