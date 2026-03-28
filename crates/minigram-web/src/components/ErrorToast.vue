<script setup>
import { useMessenger } from '../composables/useMessenger'

const { error, clearError } = useMessenger()
</script>

<template>
  <div v-if="error" class="error-toast" @click="clearError">
    {{ error }}
  </div>
</template>
