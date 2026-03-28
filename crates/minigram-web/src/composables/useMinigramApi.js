export const invokeTauri = async (cmd, args = {}) => {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke(cmd, args)
  }
  throw new Error('Tauri API is unavailable. Run this in tauri dev mode.')
}

export const formatTimestamp = (ts) => {
  if (!ts) {
    return ''
  }
  return new Date(ts * 1000).toLocaleString()
}
