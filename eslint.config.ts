import antfu from '@antfu/eslint-config'

export default antfu({
  formatters: true,
  unocss: true,
  rules: {
    'antfu/if-newline': 'off',
    'style/brace-style': ['error', '1tbs'],
    'ts/no-use-before-define': 'off',
    'unused-imports/no-unused-imports': 'error',
    'perfectionist/sort-imports': 'off',
    'import/order': [
      'error',
      {
        'newlines-between': 'always',
        'groups': ['type', 'builtin', 'external', 'internal', 'parent', 'sibling', 'index', 'object'],
        'alphabetize': {
          order: 'asc',
          caseInsensitive: true,
        },
      },
    ],
    'vue/attributes-order': ['error', { alphabetical: true }],
    'vue/max-attributes-per-line': 'error',
  },
  ignores: ['**/*.toml'],
})
