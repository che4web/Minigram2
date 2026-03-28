<script setup>
import { formatTimestamp } from '../composables/useMinigramApi'

defineProps({
  messages: {
    type: Array,
    required: true,
  },
  author: {
    type: String,
    default: 'me',
  },
  emptyText: {
    type: String,
    default: 'Нет сообщений в этом чате.',
  },
})

const toBase64 = (bytes = []) => {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i])
  }
  return btoa(binary)
}

const attachmentHref = (att) => `data:${att.mime_type};base64,${toBase64(att.data)}`
const isImage = (att) => att.mime_type?.startsWith('image/') || att.kind === 'photo'
</script>

<template>
  <div class="messages">
    <template v-if="messages.length">
      <div v-for="msg in messages" :key="msg.id" class="bubble" :class="{ outgoing: msg.author === author }">
        <div class="author">{{ msg.author }}</div>
        <div v-if="msg.text" class="text">{{ msg.text }}</div>

        <div v-if="msg.attachments?.length" class="attachments">
          <div v-for="att in msg.attachments" :key="att.id" class="attachment-item">
            <img v-if="isImage(att)" class="attachment-image" :src="attachmentHref(att)" :alt="att.file_name" />
            <a v-else class="attachment-file" :href="attachmentHref(att)" :download="att.file_name">
              📎 {{ att.file_name }}
            </a>
          </div>
        </div>

        <div class="time">{{ formatTimestamp(msg.created_at) }}</div>
      </div>
    </template>
    <div v-else class="empty">{{ emptyText }}</div>
  </div>
</template>
