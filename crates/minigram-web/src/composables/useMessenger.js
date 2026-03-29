import { computed, ref } from 'vue'
import { invokeTauri } from './useMinigramApi'

const chats = ref([])
const selectedChat = ref(null)
const messages = ref([])
const filter = ref('')
const author = ref('me')
const draft = ref('')
const queuedAttachments = ref([])
const status = ref({ pending_uploads: 0, last_sync_timestamp: 0 })
const loading = ref(false)
const error = ref('')
const jwtToken = ref('')

const JWT_STORAGE_KEY = 'minigram.jwt_token'

const filteredChats = computed(() => {
  const query = filter.value.trim().toLowerCase()
  if (!query) {
    return chats.value
  }

  return chats.value.filter((chat) => chat.chat_id.toLowerCase().includes(query))
})

const selectedMeta = computed(
  () => chats.value.find((chat) => chat.chat_id === selectedChat.value) ?? null,
)

const loadChats = async ({ selectFirst = false } = {}) => {
  loading.value = true
  error.value = ''

  try {
    const result = await invokeTauri('list_chats')
    chats.value = result.chats
    status.value = result.status

    if (selectFirst && chats.value.length > 0 && !selectedChat.value) {
      await openChat(chats.value[0].chat_id)
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

const openChat = async (chatId) => {
  selectedChat.value = chatId
  loading.value = true
  error.value = ''

  try {
    const result = await invokeTauri('list_messages', { payload: { chat: chatId } })
    messages.value = result.messages
    status.value = result.status
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

const queueFiles = async (fileList) => {
  const files = Array.from(fileList || [])
  if (files.length === 0) {
    return
  }

  const normalized = await Promise.all(files.map(async (file) => {
    const buffer = await file.arrayBuffer()
    const bytes = Array.from(new Uint8Array(buffer))
    const isImage = file.type.startsWith('image/')

    return {
      kind: isImage ? 'photo' : 'file',
      file_name: file.name,
      mime_type: file.type || (isImage ? 'image/jpeg' : 'application/octet-stream'),
      data: bytes,
      size: file.size,
    }
  }))

  queuedAttachments.value = [...queuedAttachments.value, ...normalized]
}

const removeAttachment = (index) => {
  queuedAttachments.value = queuedAttachments.value.filter((_, i) => i !== index)
}

const clearAttachments = () => {
  queuedAttachments.value = []
}

const sendMessage = async () => {
  if (!selectedChat.value || (!draft.value.trim() && queuedAttachments.value.length === 0)) {
    return false
  }

  loading.value = true
  error.value = ''

  try {
    await invokeTauri('send_message', {
      payload: {
        chat: selectedChat.value,
        author: author.value || 'me',
        text: draft.value,
        attachments: queuedAttachments.value.map(({ size, ...item }) => item),
      },
    })

    draft.value = ''
    clearAttachments()
    await openChat(selectedChat.value)
    await loadChats()
    return true
  } catch (e) {
    error.value = String(e)
    return false
  } finally {
    loading.value = false
  }
}

const syncMessages = async () => {
  loading.value = true
  error.value = ''

  try {
    await invokeTauri('sync_messages')
    await loadChats()
    if (selectedChat.value) {
      await openChat(selectedChat.value)
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

const createOrSelectChat = async () => {
  const chat = window.prompt('ID чата (например: general)')
  if (!chat) {
    return null
  }

  const normalized = chat.trim()
  if (!normalized) {
    return null
  }

  await openChat(normalized)
  await loadChats()
  return normalized
}

const clearError = () => {
  error.value = ''
}

const setJwtToken = async (value, { persist = true } = {}) => {
  const normalized = String(value ?? '').trim()

  try {
    await invokeTauri('set_jwt_token', {
      token: normalized || null,
    })

    jwtToken.value = normalized
    if (persist && typeof window !== 'undefined' && window.localStorage) {
      if (normalized) {
        window.localStorage.setItem(JWT_STORAGE_KEY, normalized)
      } else {
        window.localStorage.removeItem(JWT_STORAGE_KEY)
      }
    }
  } catch (e) {
    error.value = String(e)
    throw e
  }
}

const initJwtToken = async () => {
  if (typeof window === 'undefined' || !window.localStorage) {
    return
  }

  const stored = window.localStorage.getItem(JWT_STORAGE_KEY) ?? ''
  if (!stored.trim()) {
    jwtToken.value = ''
    return
  }

  await setJwtToken(stored, { persist: false })
}

export const useMessenger = () => ({
  chats,
  selectedChat,
  messages,
  filter,
  author,
  draft,
  queuedAttachments,
  status,
  loading,
  error,
  jwtToken,
  filteredChats,
  selectedMeta,
  loadChats,
  openChat,
  queueFiles,
  removeAttachment,
  clearAttachments,
  sendMessage,
  syncMessages,
  createOrSelectChat,
  clearError,
  setJwtToken,
  initJwtToken,
})
