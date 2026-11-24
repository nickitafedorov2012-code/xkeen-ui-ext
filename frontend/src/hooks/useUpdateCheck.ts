import { useState, useEffect } from 'react'
import { apiGet, apiPost } from '../api'
import { isUpdateAvailable } from '../utils/version'

export type UpdateChannel = 'stable' | 'beta'

export interface UpdateCheckInfo {
  current: string
  latest: string
  notes: string[]
  update_available: boolean
  channel: UpdateChannel
  asset_ready: boolean
  error?: string
}

interface UpdateCheckState {
  updateInfo: UpdateCheckInfo | null
  loading: boolean
}

let globalState: UpdateCheckState = {
  updateInfo: null,
  loading: false,
}

const listeners = new Set<(state: UpdateCheckState) => void>()
let pollTimer: ReturnType<typeof setInterval> | null = null
let inFlightPromise: Promise<void> | null = null
let activeController: AbortController | null = null

function notifyListeners() {
  for (const listener of listeners) {
    listener(globalState)
  }
}

export interface FetchUpdateOptions {
  channel?: UpdateChannel
  force?: boolean
}

async function fetchUpdateCheck(opts?: FetchUpdateOptions): Promise<void> {
  if (!opts?.force && typeof document !== 'undefined' && document.hidden) return
  if (inFlightPromise && !opts?.force && !opts?.channel) return inFlightPromise

  if (activeController && (opts?.force || opts?.channel)) {
    activeController.abort()
  }
  const controller = new AbortController()
  activeController = controller

  const params = new URLSearchParams()
  const targetChannel = opts?.channel ?? globalState.updateInfo?.channel
  if (targetChannel) params.set('channel', targetChannel)
  if (opts?.force) params.set('force', 'true')
  const qs = params.toString()
  const path = qs ? `update/check?${qs}` : 'update/check'

  if (opts?.force || opts?.channel) {
    globalState = { ...globalState, loading: true }
    notifyListeners()
  }

  const run = (async () => {
    try {
      const res = await apiGet<{
        current: string
        latest: string
        notes?: string[]
        update_available: boolean
        channel?: string
        asset_ready?: boolean
        error?: string
      }>(path, controller.signal)
      if (controller.signal.aborted) return
      if (res) {
        const cur = res.current || ''
        const lat = res.latest || ''
        const hasUpd = isUpdateAvailable(cur, lat) || Boolean(res.update_available && (cur === '' || isUpdateAvailable(cur, lat)))
        const ch: UpdateChannel = res.channel === 'beta' ? 'beta' : 'stable'
        globalState = {
          updateInfo: {
            current: cur,
            latest: lat,
            notes: res.notes || [],
            update_available: hasUpd,
            channel: ch,
            asset_ready: res.asset_ready !== false,
            error: res.error,
          },
          loading: false,
        }
        notifyListeners()
      }
    } catch {
      if (controller.signal.aborted) return
      globalState = { ...globalState, loading: false }
      notifyListeners()
    } finally {
      if (activeController === controller) {
        activeController = null
        inFlightPromise = null
      }
    }
  })()

  inFlightPromise = run
  return run
}

async function switchUpdateChannel(channel: UpdateChannel): Promise<void> {
  await apiPost('update/channel', { channel })
  await fetchUpdateCheck({ channel, force: true })
}

function startPollingIfNeeded() {
  if (!pollTimer && typeof window !== 'undefined') {
    fetchUpdateCheck()
    pollTimer = setInterval(fetchUpdateCheck, 900_000)
  }
}

function stopPollingIfEmpty() {
  if (listeners.size === 0 && pollTimer) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

export function useUpdateCheck() {
  const [state, setState] = useState<UpdateCheckState>(globalState)

  useEffect(() => {
    const handleState = (next: UpdateCheckState) => {
      setState(next)
    }
    listeners.add(handleState)
    startPollingIfNeeded()

    return () => {
      listeners.delete(handleState)
      stopPollingIfEmpty()
    }
  }, [])

  return {
    updateInfo: state.updateInfo,
    loading: state.loading,
    refresh: fetchUpdateCheck,
    setChannel: switchUpdateChannel,
  }
}
