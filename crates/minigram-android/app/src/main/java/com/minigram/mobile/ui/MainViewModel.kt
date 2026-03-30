package com.minigram.mobile.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.minigram.mobile.data.parseSyncResponse
import com.minigram.mobile.rust.MinigramNative
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

data class MainUiState(
    val chatId: String = "general",
    val author: String = "android",
    val text: String = "",
    val status: String = "Ready",
)

class MainViewModel : ViewModel() {
    private val _state = MutableStateFlow(MainUiState())
    val state: StateFlow<MainUiState> = _state.asStateFlow()

    private val serverUrl = "http://10.0.2.2:50051"
    private val dbPath = "/data/user/0/com.minigram.mobile/files/minigram_android.db"

    fun onTextChange(value: String) {
        _state.value = _state.value.copy(text = value)
    }

    fun sendAndSync() {
        val snapshot = _state.value
        if (snapshot.text.isBlank()) {
            _state.value = snapshot.copy(status = "Message is empty")
            return
        }

        viewModelScope.launch {
            runCatching {
                MinigramNative.addLocalMessage(dbPath, snapshot.chatId, snapshot.author, snapshot.text)
                MinigramNative.syncOnce(serverUrl, dbPath, "")
            }.onSuccess { raw ->
                val parsed = parseSyncResponse(raw)
                _state.value = _state.value.copy(
                    text = "",
                    status = if (parsed.ok && parsed.data != null) {
                        "Synced: +${parsed.data.pushed}/-${parsed.data.pulled}"
                    } else {
                        "Sync error: ${parsed.error ?: "unknown"}"
                    }
                )
            }.onFailure { err ->
                _state.value = _state.value.copy(status = "Native error: ${err.message}")
            }
        }
    }
}
