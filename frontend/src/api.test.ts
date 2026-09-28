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

  it.each([401, 403, 500, 503])('rejects HTTP %s even with a success envelope', async (status) => {
    global.fetch = vi.fn().mockResolvedValue({
      status,
      ok: false,
      statusText: 'Failure',
      json: async () => ({ success: true, data: { saved: true } }),
    })
    await expect(apiPost('zapret/action', { action: 'start' })).rejects.toThrow(`HTTP ${status}`)
  })

  it.each(['false', 1, {}, null])('rejects invalid success value %j', async (success) => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      ok: true,
      json: async () => ({ success, data: {} }),
    })
    await expect(apiGet('status')).rejects.toThrow('Ошибка API')
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

  it('anti-regression (API-01): apiPost auth/change-password targets /api/auth/change-password', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { saved: true, enabled: true } }),
    } as any)

    const res = await apiPost<{ saved: boolean; enabled: boolean }>('auth/change-password', {
      enabled: true,
      current_password: 'old',
      new_password: 'new',
    })
    expect(res).toEqual({ saved: true, enabled: true })
    expect(global.fetch).toHaveBeenCalledWith('/api/auth/change-password', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled: true, current_password: 'old', new_password: 'new' }),
    })
  })

  it('anti-regression (API-02): devices/domain-rules receives { rules: ... } DTO without data loss', async () => {
    const mockRules = {
      rules: {
        '192.168.1.105': [{ domain: 'example.com', target: 'DIRECT' }],
      },
    }
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: mockRules }),
    } as any)

    const res = await apiGet<{ rules: Record<string, Array<{ domain: string; target: string }>> }>('devices/domain-rules')
    expect(res.rules).toBeDefined()
    expect(res.rules['192.168.1.105']).toHaveLength(1)
    expect(res.rules['192.168.1.105'][0].domain).toBe('example.com')
  })

  it('anti-regression (UI-08): validateEndpointData rejects critical responses with invalid structure', async () => {
    // 1. config-files/read missing content
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { file: 'mihomo' } }),
    } as any)
    await expect(apiGet('config-files/read?file=mihomo')).rejects.toThrow('отсутствует обязательное поле content')

    // 2. servers missing servers array
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { foo: 'bar' } }),
    } as any)
    await expect(apiGet('servers')).rejects.toThrow('отсутствует список серверов')

    // 3. diagnostics/health missing checks array
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { ok: true } }),
    } as any)
    await expect(apiGet('diagnostics/health')).rejects.toThrow('отсутствует массив проверок checks')

    // 4. rules missing rules array
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { ok: true } }),
    } as any)
    await expect(apiGet('rules')).rejects.toThrow('отсутствует массив правил rules')
  })
})
