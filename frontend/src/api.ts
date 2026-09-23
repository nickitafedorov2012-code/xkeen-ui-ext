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
  const res = await fetch(`/api/${path.replace(/^\//, '')}`, options)
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent('xr:auth-required'))
  }
  const env: ApiEnvelope<T> = await res.json().catch(() => ({ success: false, error: `HTTP ${res.status}` }))
  if (!env.success) throw new Error(env.error || 'Ошибка API')
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


