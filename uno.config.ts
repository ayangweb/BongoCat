import { presetAntd } from '@antdv-next/unocss'
import {
  defineConfig,
  presetIcons,
  presetWind3,
  transformerDirectives,
  transformerVariantGroup,
} from 'unocss'

export default defineConfig({
  presets: [
    presetWind3(),
    presetIcons(),
    presetAntd(),
  ],
  transformers: [
    transformerVariantGroup(),
    transformerDirectives({
      applyVariable: ['--uno'],
    }),
  ],
})
