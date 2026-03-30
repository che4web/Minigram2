package com.minigram.mobile.rust

object MinigramNative {
    init {
        System.loadLibrary("minigram_mobile_ffi")
    }

    external fun syncOnce(serverUrl: String, dbPath: String, jwtToken: String): String
    external fun addLocalMessage(dbPath: String, chatId: String, author: String, text: String): String
}
