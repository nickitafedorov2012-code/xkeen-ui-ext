export interface ApiEnvelope<T> {
  success: boolean
  data?: T
  error?: string
}

export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(`/api/${path}`)
  const env: ApiEnvelope<T> = await res.json()
  if (!env.success) throw new Error(env.error || 'Ошибка API')
  return env.data as T
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  })
  const env: ApiEnvelope<T> = await res.json()
  if (!env.success) throw new Error(env.error || 'Ошибка API')
  return env.data as T
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`/api/${path}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  const env: ApiEnvelope<T> = await res.json()
  if (!env.success) throw new Error(env.error || 'Ошибка API')
  return env.data as T
}
