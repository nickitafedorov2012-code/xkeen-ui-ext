import { useState, useEffect } from 'react'
import { apiGet } from '../api'
import { isUpdateAvailable } from '../utils/version'

export interface UpdateCheckInfo {
  current: string
  latest: string
  notes: string[]
  update_available: boolean
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

function notifyListeners() {
  for (const listener of listeners) {
    listener(globalState)
  }
}

async function fetchUpdateCheck(): Promise<void> {
  if (typeof document !== 'undefined' && document.hidden) return
  if (inFlightPromise) return inFlightPromise

  inFlightPromise = (async () => {
    try {
      const res = await apiGet<{ current: string; latest: string; notes?: string[]; update_available: boolean }>('update/check')
      if (res) {
        const cur = res.current || ''
        const lat = res.latest || ''
        // Check update availability via numeric/semver comparison or backend flag validated by isUpdateAvailable
        const hasUpd = isUpdateAvailable(cur, lat) || Boolean(res.update_available && (cur === '' || isUpdateAvailable(cur, lat)))
        globalState = {
          updateInfo: {
            current: cur,
            latest: lat,
            notes: res.notes || [],
            update_available: hasUpd,
          },
          loading: false,
        }
        notifyListeners()
      }
    } catch {
      /* ignore network errors */
    } finally {
      inFlightPromise = null
    }
  })()

  return inFlightPromise
}

function startPollingIfNeeded() {
  if (!pollTimer && typeof window !== 'undefined') {
    fetchUpdateCheck()
    pollTimer = setInterval(fetchUpdateCheck, 60_000)
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
  }
}
