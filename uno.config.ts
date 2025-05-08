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
  shortcuts: [
    [/^bg-color-(\d+)$/, ([, d]) => `bg-bg-${d}`],
    [/^text-color-(\d+)$/, ([, d]) => `text-text-${d}`],
    [/^b-color-(\d+)$/, ([, d]) => `b-border-${d}`],
    [/^(.*)-primary-(\d+)$/, ([, s, d]) => `${s}-[var(--ant-blue-${d})]`],
  ],
  theme: {
    colors: {
      'bg-1': 'var(--ant-color-bg-layout)',
      'bg-2': 'var(--ant-color-bg-container)',
      'bg-3': 'var(--ant-color-bg-elevated)',
      'bg-4': 'var(--ant-color-bg-spotlight)',
      'bg-5': 'var(--ant-color-fill)',
      'bg-6': 'var(--ant-color-fill-secondary)',
      'bg-7': 'var(--ant-color-fill-tertiary)',
      'bg-8': 'var(--ant-color-fill-quaternary)',
      'text-1': 'var(--ant-color-text)',
      'text-2': 'var(--ant-color-text-secondary)',
      'text-3': 'var(--ant-color-text-tertiary)',
      'text-4': 'var(--ant-color-text-quaternary)',
      'border-1': 'var(--ant-color-border)',
      'border-2': 'var(--ant-color-border-secondary)',
      'primary': 'var(--ant-blue)',
      'success': 'var(--ant-green)',
      'danger': 'var(--ant-red)',
    },
  },
})
