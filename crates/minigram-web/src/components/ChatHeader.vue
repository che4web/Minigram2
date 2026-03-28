<script setup>
defineProps({
  title: {
    type: String,
    default: 'Выберите чат',
  },
  meta: {
    type: String,
    default: 'Слева доступен список диалогов',
  },
})
</script>

<template>
  <div class="chat-header">
    <div class="title">{{ title }}</div>
    <div class="meta">{{ meta }}</div>
  </div>
</template>
