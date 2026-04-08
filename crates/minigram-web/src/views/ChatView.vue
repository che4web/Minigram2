<script setup>
import { computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import ChatHeader from '../components/ChatHeader.vue'
import MessageList from '../components/MessageList.vue'
import ComposerBar from '../components/ComposerBar.vue'
import { useMessenger } from '../composables/useMessenger'

const route = useRoute()
const { messages, selectedMeta, selectedChatProfile, openChat, loading, author } = useMessenger()

const chatId = computed(() => String(route.params.chatId ?? ''))
const headerMeta = computed(() => {
  if (!selectedMeta.value) {
    return '0 messages'
  }

  const base = `${selectedMeta.value.message_count} messages`
  if (selectedChatProfile.value?.type === 'group') {
    const members = selectedChatProfile.value.members?.length
      ? ` · ${selectedChatProfile.value.members.join(', ')}`
      : ''
    return `${base} · групповой чат${members}`
  }

  return `${base} · личный чат`
})

const headerTitle = computed(() => selectedChatProfile.value?.title || chatId.value)

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
    <ChatHeader :title="headerTitle" :meta="headerMeta" />
    <MessageList :messages="messages" :author="author" />
    <ComposerBar :disabled="loading" />
  </div>
</template>
