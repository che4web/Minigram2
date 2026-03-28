import { createRouter, createWebHashHistory } from 'vue-router'
import ChatsView from '../views/ChatsView.vue'
import ChatView from '../views/ChatView.vue'

const routes = [
  {
    path: '/',
    redirect: '/chats',
  },
  {
    path: '/chats',
    name: 'chats',
    component: ChatsView,
  },
  {
    path: '/chats/:chatId',
    name: 'chat',
    component: ChatView,
  },
]

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
})
