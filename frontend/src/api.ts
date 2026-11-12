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
  if (data === undefined) {
    throw new Error(`Эндпоинт /api/${base} вернул некорректный ответ (отсутствует поле data)`)
  }
  if (data === null) {
    return
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

const inflightPosts = new Map<string, Promise<unknown>>()

function getInflightPostKey(cleanPath: string, options: RequestInit): string | null {
  const method = (options.method ?? 'GET').toUpperCase()
  if (method !== 'POST') return null
  const body = typeof options.body === 'string' ? options.body : ''
  return `${method}:${cleanPath}:${body}`
}

async function apiFetch<T>(path: string, options: RequestInit = {}): Promise<T> {
  const cleanPath = path.replace(/^\/+/, '')
  const inflightKey = getInflightPostKey(cleanPath, options)
  if (inflightKey) {
    const existing = inflightPosts.get(inflightKey)
    if (existing) {
      return existing as Promise<T>
    }
  }

  const run = (async (): Promise<T> => {
    const headers = new Headers(options.headers || {})
    if (!headers.has('X-Requested-With')) {
      headers.set('X-Requested-With', 'XMLHttpRequest')
    }
    // FE-02: зависший бэкенд раньше означал вечный pending — добавляем таймаут 30с
    const controller = new AbortController()
    const abortTimer = setTimeout(() => controller.abort(), 30000)
    try {
      const res = await fetch(`/api/${cleanPath}`, {
        ...options,
        headers,
        signal: controller.signal,
      })
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
    } catch (err) {
      if (controller.signal.aborted) {
        const message = 'Роутер не ответил за 30 сек.'
        const method = options.method?.toUpperCase() ?? 'GET'
        throw new Error(method === 'GET' ? message : `${message} Результат запроса пока неизвестен.`)
      }
      throw err
    } finally {
      clearTimeout(abortTimer)
      if (inflightKey) {
        inflightPosts.delete(inflightKey)
      }
    }
  })()

  if (inflightKey) {
    inflightPosts.set(inflightKey, run)
  }

  return run
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

/**
 * Ручной запуск проверки детектора белых списков
 */
export async function runWhitelistDetectorCheck(): Promise<{ message: string }> {
  return apiPost<{ message: string }>('failover/whitelist-detector/check', {})
}

/**
 * Локальное определение и классификация серверов и подписок белых списков.
 * Использует признак is_whitelist от бэкенда; не дублирует проверку тегов на фронтенде.
 */
export function detectWhitelistServers(
  servers: Array<{ id: string; name: string; provider?: string; is_whitelist?: boolean }>,
  providers: Array<{ id: string; name: string; is_whitelist?: boolean }>
): {
  detectedServerIds: string[]
  detectedProviderIds: string[]
  details: Array<{ id: string; name: string; reason: string; type: 'server' | 'provider' }>
} {
  const detectedServerIds: string[] = []
  const detectedProviderIds: string[] = []
  const details: Array<{ id: string; name: string; reason: string; type: 'server' | 'provider' }> = []

  for (const prov of providers) {
    if (prov.is_whitelist) {
      if (!detectedProviderIds.includes(prov.id)) {
        detectedProviderIds.push(prov.id)
        details.push({
          id: prov.id,
          name: prov.name,
          reason: 'Подписка БС (по данным сервера)',
          type: 'provider',
        })
      }
    }
  }

  for (const srv of servers) {
    if (srv.is_whitelist) {
      if (!detectedServerIds.includes(srv.id)) {
        detectedServerIds.push(srv.id)
        details.push({
          id: srv.id,
          name: srv.name,
          reason: 'Сервер БС (по данным сервера)',
          type: 'server',
        })
      }
    }
  }

  return { detectedServerIds, detectedProviderIds, details }
}

/**
 * Применение определенных БС узлов и подписок в 1 клик
 */
export async function applyDetectedWhitelistServers(
  serverIds: string[],
  providerIds: string[]
): Promise<{ whitelist_servers: string[]; whitelist_providers: string[] }> {
  let resServers: string[] = []
  let resProviders: string[] = []

  if (serverIds.length > 0) {
    // Маршрут бэкенда — /api/servers/whitelist (раньше слали в несуществующий whitelist/servers)
    const srvRes = await apiPost<{ whitelist_servers: string[]; whitelist_providers: string[] }>(
      'servers/whitelist',
      // Баг 98: найденные узлы добавляются к ручным пометкам, а не заменяют их
      { add_servers: serverIds }
    )
    resServers = srvRes.whitelist_servers || serverIds
    if (srvRes.whitelist_providers) resProviders = srvRes.whitelist_providers
  }

  if (providerIds.length > 0) {
    const provRes = await apiPost<{ whitelist_servers: string[]; whitelist_providers: string[] }>(
      'whitelist/providers',
      { add_providers: providerIds }
    )
    resProviders = provRes.whitelist_providers || providerIds
    if (provRes.whitelist_servers) resServers = provRes.whitelist_servers
  }

  return { whitelist_servers: resServers, whitelist_providers: resProviders }
}

// ============================================================================
// VK-туннель (VK-turn) API
// ============================================================================

export async function getVkTurnStatus(): Promise<import('./types').VkTurnStatus> {
  return apiGet<import('./types').VkTurnStatus>('vkturn/status')
}

export async function getVkTurnLogs(): Promise<{ lines: string[] }> {
  return apiGet<{ lines: string[] }>('vkturn/logs')
}

export async function saveVkTurnConfig(
  payload: import('./types').VkTurnConfigPayload
): Promise<{ success: boolean; message?: string }> {
  return apiPost<{ success: boolean; message?: string }>('vkturn/config', payload)
}

export async function vkTurnAction(
  action: string
): Promise<import('./types').VkTurnActionResponse> {
  return apiPost<import('./types').VkTurnActionResponse>('vkturn/action', { action })
}

export async function saveVkTurnAuth(payload: {
  token?: string
  access_token?: string
  url?: string
  user_id?: number
  auth_mode?: string
}): Promise<{ success: boolean; user_id?: number; user_name?: string; message?: string }> {
  return apiPost<{ success: boolean; user_id?: number; user_name?: string; message?: string }>(
    'vkturn/auth/save',
    payload
  )
}

export async function logoutVkTurnAuth(): Promise<{ success: boolean; message?: string }> {
  return apiPost<{ success: boolean; message?: string }>('vkturn/auth/logout', {})
}

