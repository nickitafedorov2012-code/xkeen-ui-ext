export interface ApiEnvelope<T> {
  success: boolean
  data?: T
  error?: string
  auth_required?: boolean
}

export function getWsUrl(path: string): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${protocol}//${window.location.host}/api/${path.replace(/^\//, '')}`
}

async function apiFetch<T>(path: string, options: RequestInit = {}): Promise<T> {
  const cleanPath = path.replace(/^\//, '')
  const res = await fetch(`/api/${cleanPath}`, options)
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent('xr:auth-required'))
  }
  if (res.status === 204) {
    return {} as T
  }
  const ct = res.headers?.get?.('content-type') || ''
  if (ct && !ct.includes('application/json') && !ct.includes('text/json')) {
    if (res.ok) {
      throw new Error(`Эндпоинт /api/${cleanPath} недоступен на текущей версии бэкенда`)
    }
    throw new Error(`HTTP ${res.status}: ${res.statusText || 'Ошибка сервера'}`)
  }
  let env: ApiEnvelope<T>
  try {
    env = await res.json()
  } catch {
    if (res.ok) {
      throw new Error(`Эндпоинт /api/${cleanPath} вернул некорректный ответ`)
    }
    throw new Error(`HTTP ${res.status}: ${res.statusText || 'Ошибка сервера'}`)
  }
  if (!env || !env.success) throw new Error(env?.error || 'Ошибка API')
  return env.data as T
}

export async function apiGet<T>(path: string): Promise<T> {
  return apiFetch<T>(path)
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  })
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
}

export async function apiDelete<T>(path: string, body?: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
}


