<script setup>
import ChatHeader from '../components/ChatHeader.vue'
import ComposerBar from '../components/ComposerBar.vue'
</script>

<template>
  <div class="main">
    <ChatHeader title="Выберите чат" meta="Слева доступен список диалогов" />
    <div class="empty">Откройте чат из списка или создайте новый.</div>
    <ComposerBar :disabled="true" />
  </div>
</template>
