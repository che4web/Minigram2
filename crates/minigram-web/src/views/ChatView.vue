<script setup>
import { computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import ChatHeader from '../components/ChatHeader.vue'
import MessageList from '../components/MessageList.vue'
import ComposerBar from '../components/ComposerBar.vue'
import { useMessenger } from '../composables/useMessenger'

const route = useRoute()
const { messages, selectedMeta, openChat, loading, author } = useMessenger()

const chatId = computed(() => String(route.params.chatId ?? ''))
const headerMeta = computed(() => {
  if (!selectedMeta.value) {
    return '0 messages'
  }
  return `${selectedMeta.value.message_count} messages`
})

const loadRouteChat = async () => {
  if (!chatId.value) {
    return
  }
  await openChat(chatId.value)
}

onMounted(loadRouteChat)
watch(chatId, loadRouteChat)
</script>

<template>
  <div class="main">
    <ChatHeader :title="chatId" :meta="headerMeta" />
    <MessageList :messages="messages" :author="author" />
    <ComposerBar :disabled="loading" />
  </div>
</template>
