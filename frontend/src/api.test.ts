import { describe, it, expect, vi, beforeEach } from 'vitest'
import { apiGet, apiPost, apiPut, apiDelete, getWsUrl } from './api'

describe('api client', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('apiGet unwraps envelope data on success', async () => {
    const mockData = { version: '1.2.6', status: 'ok' }
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: mockData }),
    } as any)

    const res = await apiGet<typeof mockData>('status')
    expect(res).toEqual(mockData)
    expect(global.fetch).toHaveBeenCalledWith('/api/status', {})
  })

  it('apiGet handles HTTP 204 No Content without json parse', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 204,
      json: vi.fn(),
    } as any)

    const res = await apiGet<any>('settings/reset')
    expect(res).toEqual({})
  })

  it('apiGet throws error when success is false', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 400,
      json: async () => ({ success: false, error: 'Неверные параметры запроса' }),
    } as any)

    await expect(apiGet('invalid')).rejects.toThrow('Неверные параметры запроса')
  })

  it('apiGet dispatches xr:auth-required event on 401 Unauthorized', async () => {
    let authEventTriggered = false
    window.addEventListener('xr:auth-required', () => {
      authEventTriggered = true
    }, { once: true })

    global.fetch = vi.fn().mockResolvedValue({
      status: 401,
      json: async () => ({ success: false, error: 'Unauthorized' }),
    } as any)

    try {
      await apiGet('protected')
    } catch {
      // Ожидаемый выброс ошибки
    }

    expect(authEventTriggered).toBe(true)
  })

  it('apiPost sends POST request with json body', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { switched: true } }),
    } as any)

    await apiPost('servers/switch', { server_id: 'srv1' })
    expect(global.fetch).toHaveBeenCalledWith('/api/servers/switch', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ server_id: 'srv1' }),
    })
  })

  it('apiPut sends PUT request with json body', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { updated: true } }),
    } as any)

    await apiPut('settings', { theme: 'dark' })
    expect(global.fetch).toHaveBeenCalledWith('/api/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ theme: 'dark' }),
    })
  })

  it('apiDelete sends DELETE request', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { deleted: true } }),
    } as any)

    await apiDelete('backups/delete', { name: 'old_backup' })
    expect(global.fetch).toHaveBeenCalledWith('/api/backups/delete', {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'old_backup' }),
    })
  })

  it('getWsUrl converts http/https host to ws/wss', () => {
    expect(getWsUrl('logs/ws')).toContain('/api/logs/ws')
  })
})
