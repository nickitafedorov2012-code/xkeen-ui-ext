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

/**
 * UI-08: Валидация структуры полезной нагрузки (data payload) для критических эндпоинтов API.
 * Защищает UI от скрытых ошибок при нарушении контрактов данных даже при success === true.
 */
export function validateEndpointData(cleanPath: string, data: any): void {
  const base = cleanPath.split('?')[0].replace(/^\/+|\/+$/g, '')
  if (data === null || data === undefined) {
    throw new Error(`Эндпоинт /api/${base} вернул пустые данные (null/undefined)`)
  }
  if (base === 'config-files/read') {
    if (typeof data !== 'object' || typeof data.content !== 'string') {
      throw new Error(`Некорректная структура данных в ответе /api/${base}: отсутствует обязательное поле content`)
    }
  } else if (base === 'servers') {
    if (typeof data !== 'object' || (!Array.isArray(data.servers) && !Array.isArray(data))) {
      throw new Error(`Некорректная структура данных в ответе /api/${base}: отсутствует список серверов`)
    }
  } else if (base === 'diagnostics/health') {
    if (typeof data !== 'object' || !Array.isArray(data.checks)) {
      throw new Error(`Некорректная структура данных в ответе /api/${base}: отсутствует массив проверок checks`)
    }
  } else if (base === 'rules') {
    if (typeof data !== 'object' || !Array.isArray(data.rules)) {
      throw new Error(`Некорректная структура данных в ответе /api/${base}: отсутствует массив правил rules`)
    }
  } else if (base === 'antigravity/status') {
    if (typeof data !== 'object' || typeof data.enabled !== 'boolean') {
      throw new Error(`Некорректная структура данных в ответе /api/${base}: некорректный статус Antigravity`)
    }
  }
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
  const serverError = typeof env?.error === 'string' && env.error ? env.error : undefined
  if (res.status < 200 || res.status >= 300) {
    throw new Error(serverError || `HTTP ${res.status}: ${res.statusText || 'Ошибка сервера'}`)
  }
  if (!env || env.success !== true) throw new Error(serverError || 'Ошибка API')

  // UI-08: Runtime validation полезной нагрузки критических ответов
  validateEndpointData(cleanPath, env.data)

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
