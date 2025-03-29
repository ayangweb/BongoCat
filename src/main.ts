import { createPlugin } from '@tauri-store/pinia'
import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from './App.vue'
import router from './router'
import 'virtual:uno.css'
import '@unocss/reset/tailwind-compat.css'
import './assets/css/global.css'

const pinia = createPinia()
pinia.use(createPlugin({ saveOnChange: true }))

createApp(App).use(router).use(pinia).mount('#app')
