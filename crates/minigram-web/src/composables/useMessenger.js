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
const chatMeta = ref({})
const customChats = ref([])

const JWT_STORAGE_KEY = 'minigram.jwt_token'
const CHAT_META_STORAGE_KEY = 'minigram.chat_meta'
const CUSTOM_CHATS_STORAGE_KEY = 'minigram.custom_chats'

const persistChatData = () => {
  if (typeof window === 'undefined' || !window.localStorage) {
    return
  }

  window.localStorage.setItem(CHAT_META_STORAGE_KEY, JSON.stringify(chatMeta.value))
  window.localStorage.setItem(CUSTOM_CHATS_STORAGE_KEY, JSON.stringify(customChats.value))
}

const resolveChatMeta = (chatId) => {
  const meta = chatMeta.value[chatId]
  if (meta) {
    return meta
  }

  if (chatId.startsWith('group:')) {
    return { type: 'group', title: chatId.replace(/^group:/, '') }
  }

  return { type: 'direct', title: chatId }
}

const filteredChats = computed(() => {
  const query = filter.value.trim().toLowerCase()
  if (!query) {
    return chats.value
  }

  return chats.value.filter((chat) => {
    const meta = resolveChatMeta(chat.chat_id)
    const members = (meta.members ?? []).join(' ')
    return [chat.chat_id, meta.title, members].join(' ').toLowerCase().includes(query)
  })
})

const directChats = computed(() => filteredChats.value.filter((chat) => resolveChatMeta(chat.chat_id).type === 'direct'))
const groupChats = computed(() => filteredChats.value.filter((chat) => resolveChatMeta(chat.chat_id).type === 'group'))

const selectedMeta = computed(
  () => chats.value.find((chat) => chat.chat_id === selectedChat.value) ?? null,
)

const selectedChatProfile = computed(() => {
  if (!selectedChat.value) {
    return null
  }

  return resolveChatMeta(selectedChat.value)
})

const mergeServerChats = (serverChats = []) => {
  const known = new Set(serverChats.map((chat) => chat.chat_id))
  const virtualChats = customChats.value
    .filter((chat) => !known.has(chat.chat_id))
    .map((chat) => ({
      chat_id: chat.chat_id,
      message_count: 0,
      last_message_at: 0,
      last_message_preview: 'Новый чат',
    }))

  chats.value = [...serverChats, ...virtualChats]
}

const rememberChat = (chatId, meta = null) => {
  if (!customChats.value.find((chat) => chat.chat_id === chatId)) {
    customChats.value = [{ chat_id: chatId }, ...customChats.value]
  }

  if (meta) {
    chatMeta.value = {
      ...chatMeta.value,
      [chatId]: {
        ...chatMeta.value[chatId],
        ...meta,
      },
    }
  }

  persistChatData()
}

const loadChats = async ({ selectFirst = false } = {}) => {
  loading.value = true
  error.value = ''

  try {
    const result = await invokeTauri('list_chats')
    mergeServerChats(result.chats)
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
    rememberChat(chatId)
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
  const chat = window.prompt('ID личного чата (например: alice)')
  if (!chat) {
    return null
  }

  const normalized = chat.trim()
  if (!normalized) {
    return null
  }

  rememberChat(normalized, { type: 'direct', title: normalized })
  await openChat(normalized)
  await loadChats()
  return normalized
}

const createGroupChat = async () => {
  const title = window.prompt('Название группового чата (например: Проект Alpha)')
  if (!title) {
    return null
  }

  const normalizedTitle = title.trim()
  if (!normalizedTitle) {
    return null
  }

  const membersInput = window.prompt('Участники через запятую (например: alice, bob)') ?? ''
  const members = membersInput
    .split(',')
    .map((value) => value.trim())
    .filter(Boolean)

  const slug = normalizedTitle
    .toLowerCase()
    .replace(/[^a-zа-я0-9]+/gi, '-')
    .replace(/^-+|-+$/g, '') || 'group'
  let chatId = `group:${slug}`
  let postfix = 1
  while (chatMeta.value[chatId] || customChats.value.find((chat) => chat.chat_id === chatId)) {
    postfix += 1
    chatId = `group:${slug}-${postfix}`
  }

  rememberChat(chatId, {
    type: 'group',
    title: normalizedTitle,
    members,
  })

  await openChat(chatId)
  await loadChats()
  return chatId
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

const initChatCatalog = () => {
  if (typeof window === 'undefined' || !window.localStorage) {
    return
  }

  try {
    chatMeta.value = JSON.parse(window.localStorage.getItem(CHAT_META_STORAGE_KEY) ?? '{}')
  } catch {
    chatMeta.value = {}
  }

  try {
    const parsed = JSON.parse(window.localStorage.getItem(CUSTOM_CHATS_STORAGE_KEY) ?? '[]')
    customChats.value = Array.isArray(parsed) ? parsed : []
  } catch {
    customChats.value = []
  }
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
  directChats,
  groupChats,
  selectedMeta,
  selectedChatProfile,
  loadChats,
  openChat,
  queueFiles,
  removeAttachment,
  clearAttachments,
  sendMessage,
  syncMessages,
  createOrSelectChat,
  createGroupChat,
  clearError,
  setJwtToken,
  initJwtToken,
  initChatCatalog,
  resolveChatMeta,
})
