<script setup>
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { formatTimestamp } from '../composables/useMinigramApi'
import { useMessenger } from '../composables/useMessenger'

const route = useRoute()
const router = useRouter()

const {
  filteredChats,
  filter,
  status,
  loading,
  syncMessages,
  createOrSelectChat,
  jwtToken,
  setJwtToken,
} = useMessenger()

const jwtDraft = ref(jwtToken.value)

watch(jwtToken, (value) => {
  jwtDraft.value = value
})

const activeChatId = computed(() => route.params.chatId ?? null)

const openChat = async (chatId) => {
  await router.push({ name: 'chat', params: { chatId } })
}

const openPrompt = async () => {
  const chatId = await createOrSelectChat()
  if (chatId) {
    await router.push({ name: 'chat', params: { chatId } })
  }
}

const saveJwtToken = async () => {
  await setJwtToken(jwtDraft.value)
}

const clearJwtToken = async () => {
  jwtDraft.value = ''
  await setJwtToken('')
}
</script>

<template>
  <aside class="sidebar">
    <div class="header">Minigram · Telegram style</div>

    <div class="actions">
      <input v-model="filter" class="search" placeholder="Поиск чатов..." />
      <button class="button" :disabled="loading" @click="syncMessages">Sync</button>
    </div>

    <div class="actions actions-single">
      <button class="button primary" @click="openPrompt">Новый / открыть чат</button>
    </div>

    <div class="token-panel">
      <label class="token-label" for="jwt-token">JWT token</label>
      <textarea
        id="jwt-token"
        v-model="jwtDraft"
        class="token-input"
        placeholder="Bearer token for sync"
        rows="4"
        spellcheck="false"
      />
      <div class="token-actions">
        <button class="button" :disabled="loading" @click="saveJwtToken">Сохранить</button>
        <button class="button ghost" :disabled="loading || !jwtDraft" @click="clearJwtToken">
          Очистить
        </button>
      </div>
    </div>

    <div class="chat-list">
      <div
        v-for="chat in filteredChats"
        :key="chat.chat_id"
        class="chat-item"
        :class="{ active: chat.chat_id === activeChatId }"
        @click="openChat(chat.chat_id)"
      >
        <div class="chat-row">
          <div class="chat-title">{{ chat.chat_id }}</div>
          <div class="chat-time">{{ formatTimestamp(chat.last_message_at) }}</div>
        </div>
        <div class="chat-preview">{{ chat.last_message_preview }}</div>
      </div>
    </div>

    <div class="status">
      pending: {{ status.pending_uploads }} · last_sync: {{ status.last_sync_timestamp }}
    </div>
  </aside>
</template>
