import { createPlugin } from '@tauri-store/pinia'
import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from './App.vue'
import { i18n } from './i18n'
import router from './router'
import 'virtual:uno.css'
import 'ant-design-vue/dist/reset.css'
import './assets/css/global.scss'

const pinia = createPinia()
pinia.use(createPlugin({ saveOnChange: true }))

const app = createApp(App)
// Use i18n
app.use(router).use(pinia).use(i18n)
app.mount('#app')
