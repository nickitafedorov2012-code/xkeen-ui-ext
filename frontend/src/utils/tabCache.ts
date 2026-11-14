import { useCallback, useEffect, useRef, useState } from 'react'

const tabDataCache = new Map<string, unknown>()

export function clearTabCache(): void {
  tabDataCache.clear()
}

export function useTabCache<T>(key: string, fetcher: () => Promise<T>): {
  data: T | undefined
  loading: boolean
  error: unknown
  refresh: () => Promise<void>
} {
  const [data, setData] = useState<T | undefined>(() =>
    tabDataCache.has(key) ? (tabDataCache.get(key) as T) : undefined
  )
  const [loading, setLoading] = useState<boolean>(() => !tabDataCache.has(key))
  const [error, setError] = useState<unknown>(null)
  const fetcherRef = useRef(fetcher)
  fetcherRef.current = fetcher

  const refresh = useCallback(async () => {
    if (!tabDataCache.has(key)) {
      setLoading(true)
    }
    try {
      const fresh = await fetcherRef.current()
      tabDataCache.set(key, fresh)
      setData(fresh)
      setError(null)
    } catch (err) {
      setError(err)
    } finally {
      setLoading(false)
    }
  }, [key])

  useEffect(() => {
    if (tabDataCache.has(key)) {
      setData(tabDataCache.get(key) as T)
      setLoading(false)
    }
    void refresh()
  }, [key, refresh])

  return { data, loading, error, refresh }
}
