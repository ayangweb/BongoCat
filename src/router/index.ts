// @unocss-include
import type { RouteRecordRaw } from 'vue-router'

import { createRouter, createWebHashHistory } from 'vue-router'

import Preference from '../layouts/preference/index.vue'
import General from '../pages/general/index.vue'
import Main from '../pages/main/index.vue'

export const preferenceRoutes: RouteRecordRaw[] = [
  {
    path: 'cat',
    component: () => import('../pages/cat/index.vue'),
    meta: {
      title: '猫咪设置',
      icon: 'i-solar:cat-outline',
    },
  },
  {
    path: 'general',
    component: General,
    meta: {
      title: '通用设置',
      icon: 'i-solar:settings-outline',
    },
  },
  {
    path: 'model',
    component: () => import('../pages/model/index.vue'),
    meta: {
      title: '模型管理',
      icon: 'i-solar:magic-stick-3-outline',
    },
  },
  {
    path: 'about',
    component: () => import('../pages/about/index.vue'),
    meta: {
      title: '关于',
      icon: 'i-solar:info-circle-outline',
    },
  },
]

const routes: Readonly<RouteRecordRaw[]> = [
  {
    path: '/',
    component: Main,
  },
  {
    path: '/preference',
    component: Preference,
    redirect: '/preference/cat',
    children: preferenceRoutes,
  },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export default router
