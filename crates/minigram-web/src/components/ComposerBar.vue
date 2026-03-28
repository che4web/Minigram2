<script setup>
import { computed } from 'vue'
import { useMessenger } from '../composables/useMessenger'

const props = defineProps({
  disabled: {
    type: Boolean,
    default: false,
  },
})

const {
  author,
  draft,
  loading,
  queuedAttachments,
  queueFiles,
  removeAttachment,
  sendMessage,
} = useMessenger()

const canSend = computed(
  () => !props.disabled && (!loading.value && (!!draft.value.trim() || queuedAttachments.value.length > 0)),
)

const onFileInput = async (event) => {
  await queueFiles(event.target.files)
  event.target.value = ''
}
</script>

<template>
  <div class="composer-wrap">
    <div v-if="queuedAttachments.length" class="queued-attachments">
      <div v-for="(att, idx) in queuedAttachments" :key="`${att.file_name}-${idx}`" class="queued-item">
        <span>📎 {{ att.file_name }} ({{ Math.ceil(att.size / 1024) }} KB)</span>
        <button class="button ghost" type="button" @click="removeAttachment(idx)">×</button>
      </div>
    </div>

    <form class="composer" @submit.prevent="sendMessage">
      <input v-model="author" class="input" placeholder="Ваше имя" />
      <input
        v-model="draft"
        class="input"
        :disabled="disabled"
        placeholder="Введите сообщение..."
      />
      <label class="button" :class="{ disabled }">
        Файл/Фото
        <input type="file" multiple hidden :disabled="disabled" @change="onFileInput" />
      </label>
      <button class="button primary" :disabled="!canSend">Отправить</button>
    </form>
  </div>
</template>
