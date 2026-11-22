import type { RouteRecordRaw } from 'vue-router'

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    component: () => import('../pages/MiniKmbPage.vue')
  },
  {
    path: '/channel/:uid',
    name: 'mini-control',
    component: () => import('../pages/MiniKmbPage.vue')
  }
]

export default routes

