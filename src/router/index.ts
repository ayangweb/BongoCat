import { createRouter, createWebHashHistory } from 'vue-router'

import Main from '../pages/main.vue'
import Preference from '../pages/preference.vue'

const routes = [
  {
    path: '/',
    component: Main,
  },
  {
    path: '/preference',
    component: Preference,
  },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export default router
