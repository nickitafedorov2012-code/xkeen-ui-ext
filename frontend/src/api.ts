export interface ApiEnvelope<T> {
  success: boolean
  data?: T
  error?: string
  auth_required?: boolean
}

export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(`/api/${path}`)
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent('xr:auth-required'))
  }
  const env: ApiEnvelope<T> = await res.json().catch(() => ({ success: false, error: `HTTP ${res.status}` }))
  if (!env.success) throw new Error(env.error || 'Ошибка API')
  return env.data as T
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  })
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent('xr:auth-required'))
  }
  const env: ApiEnvelope<T> = await res.json().catch(() => ({ success: false, error: `HTTP ${res.status}` }))
  if (!env.success) throw new Error(env.error || 'Ошибка API')
  return env.data as T
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`/api/${path}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent('xr:auth-required'))
  }
  const env: ApiEnvelope<T> = await res.json().catch(() => ({ success: false, error: `HTTP ${res.status}` }))
  if (!env.success) throw new Error(env.error || 'Ошибка API')
  return env.data as T
}

export async function apiDelete<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/${path}`, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  })
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent('xr:auth-required'))
  }
  const env: ApiEnvelope<T> = await res.json().catch(() => ({ success: false, error: `HTTP ${res.status}` }))
  if (!env.success) throw new Error(env.error || 'Ошибка API')
  return env.data as T
}

