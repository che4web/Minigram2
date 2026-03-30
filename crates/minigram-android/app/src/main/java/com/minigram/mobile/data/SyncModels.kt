package com.minigram.mobile.data

import org.json.JSONObject

data class SyncStats(
    val pushed: Int,
    val pulled: Int,
    val serverTimestamp: Long,
)

data class NativeResponse<T>(
    val ok: Boolean,
    val data: T?,
    val error: String?,
)

fun parseSyncResponse(raw: String): NativeResponse<SyncStats> {
    val root = JSONObject(raw)
    val ok = root.optBoolean("ok", false)
    val error = root.optString("error").ifBlank { null }
    val dataJson = root.optJSONObject("data")
    val data = if (dataJson == null) {
        null
    } else {
        SyncStats(
            pushed = dataJson.optInt("pushed", 0),
            pulled = dataJson.optInt("pulled", 0),
            serverTimestamp = dataJson.optLong("server_timestamp", 0L),
        )
    }
    return NativeResponse(ok = ok, data = data, error = error)
}
