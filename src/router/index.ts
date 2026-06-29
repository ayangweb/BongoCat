import type { RouteRecordRaw } from 'vue-router'

import { createRouter, createWebHashHistory } from 'vue-router'

import Chat from '../pages/chat/index.vue'
import Main from '../pages/main/index.vue'
import Preference from '../pages/preference/index.vue'

const routes: Readonly<RouteRecordRaw[]> = [
  {
    path: '/',
    component: Main,
  },
  {
    path: '/preference',
    component: Preference,
  },
  {
    path: '/chat',
    component: Chat,
  },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export default router
