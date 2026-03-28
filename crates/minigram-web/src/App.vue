<script setup>
import { onMounted } from 'vue'
import SidebarChats from './components/SidebarChats.vue'
import ErrorToast from './components/ErrorToast.vue'
import { useMessenger } from './composables/useMessenger'

const { loadChats } = useMessenger()

onMounted(async () => {
  await loadChats({ selectFirst: false })
})
</script>

<template>
  <div class="app">
    <SidebarChats />
    <RouterView />
  </div>
  <ErrorToast />
</template>
