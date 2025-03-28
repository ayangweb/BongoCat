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
  ],
  transformers: [
    transformerVariantGroup(),
    transformerDirectives({
      applyVariable: ['--uno'],
    }),
  ],
})
