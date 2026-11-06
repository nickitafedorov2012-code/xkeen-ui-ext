import { describe, it, expect, vi, beforeEach } from 'vitest'
import { apiGet, apiPost, apiPut, apiDelete, getWsUrl, applyDetectedWhitelistServers } from './api'

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
    expect(global.fetch).toHaveBeenCalledWith('/api/status', expect.objectContaining({
      headers: expect.any(Headers),
    }))
    const callHeaders = (global.fetch as any).mock.calls[0][1].headers as Headers
    expect(callHeaders.get('X-Requested-With')).toBe('XMLHttpRequest')
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
    expect(global.fetch).toHaveBeenCalledWith('/api/servers/switch', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({ server_id: 'srv1' }),
    }))
    const callHeaders = (global.fetch as any).mock.calls[0][1].headers as Headers
    expect(callHeaders.get('Content-Type')).toBe('application/json')
    expect(callHeaders.get('X-Requested-With')).toBe('XMLHttpRequest')
  })

  it('apiPut sends PUT request with json body', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { updated: true } }),
    } as any)

    await apiPut('settings', { theme: 'dark' })
    expect(global.fetch).toHaveBeenCalledWith('/api/settings', expect.objectContaining({
      method: 'PUT',
      body: JSON.stringify({ theme: 'dark' }),
    }))
    const callHeaders = (global.fetch as any).mock.calls[0][1].headers as Headers
    expect(callHeaders.get('Content-Type')).toBe('application/json')
    expect(callHeaders.get('X-Requested-With')).toBe('XMLHttpRequest')
  })

  it('apiDelete sends DELETE request', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: { deleted: true } }),
    } as any)

    await apiDelete('backups/delete', { name: 'old_backup' })
    expect(global.fetch).toHaveBeenCalledWith('/api/backups/delete', expect.objectContaining({
      method: 'DELETE',
      body: JSON.stringify({ name: 'old_backup' }),
    }))
    const callHeaders = (global.fetch as any).mock.calls[0][1].headers as Headers
    expect(callHeaders.get('Content-Type')).toBe('application/json')
    expect(callHeaders.get('X-Requested-With')).toBe('XMLHttpRequest')
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
    expect(global.fetch).toHaveBeenCalledWith('/api/auth/change-password', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({ enabled: true, current_password: 'old', new_password: 'new' }),
    }))
    const callHeaders = (global.fetch as any).mock.calls[0][1].headers as Headers
    expect(callHeaders.get('Content-Type')).toBe('application/json')
    expect(callHeaders.get('X-Requested-With')).toBe('XMLHttpRequest')
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

  it('anti-regression (FE-03): notifications/test validates results structure and detects failures', async () => {
    const mockSuccess = {
      results: ['Telegram: ✓ Сообщение успешно отправлено'],
      message: 'Telegram: ✓ Сообщение успешно отправлено',
    }
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: mockSuccess }),
    } as any)

    const resSuccess = await apiPost<{ results: string[]; message: string }>('notifications/test', {})
    expect(resSuccess.results).toHaveLength(1)
    expect(resSuccess.results.some((r) => r.includes('❌'))).toBe(false)

    const mockFailure = {
      success: false,
      error: 'Telegram: ❌ Chat not found',
    }
    global.fetch = vi.fn().mockResolvedValue({
      status: 400,
      json: async () => mockFailure,
    } as any)

    await expect(apiPost('notifications/test', {})).rejects.toThrow('Chat not found')
  })

  it('anti-regression (API-01): allows legitimate null data payloads when success is true', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true, data: null }),
    } as any)

    const res = await apiGet<any>('settings/optional-field')
    expect(res).toBeNull()
  })

  it('anti-regression (API-01): rejects when data is undefined even if success is true', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ success: true }),
    } as any)

    await expect(apiGet<any>('some-endpoint')).rejects.toThrow('отсутствует поле data')
  })
})

describe('regression bug 98', () => {
  it('apply detected whitelist adds to user marks instead of replacing lists', async () => {
    // Баг 98: «Применить найденные БС в 1 клик» отправляло полный список — ручные пометки стирались.
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockImplementation(async () =>
      new Response(JSON.stringify({ success: true, data: { whitelist_servers: ['manual', 'found'], whitelist_providers: ['p-manual', 'p-found'] } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    )
    const res = await applyDetectedWhitelistServers(['found'], ['p-found'])
    const bodies = fetchSpy.mock.calls.map((c) => JSON.parse(String(c[1]?.body ?? '{}')))
    expect(bodies[0]).toEqual({ add_servers: ['found'] })
    expect(bodies[1]).toEqual({ add_providers: ['p-found'] })
    expect(bodies.some((b) => 'servers' in b || 'providers' in b)).toBe(false)
    expect(res.whitelist_servers).toEqual(['manual', 'found'])
    fetchSpy.mockRestore()
  })
})

describe('VK-turn API endpoints', () => {
  it('calls getVkTurnStatus, getVkTurnLogs, saveVkTurnConfig and vkTurnAction correctly', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockImplementation(async (url: any) => {
      const u = String(url)
      if (u.includes('/api/vkturn/status')) {
        return new Response(JSON.stringify({ success: true, data: { installed: true, phase: 'connected' } }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      }
      if (u.includes('/api/vkturn/logs')) {
        return new Response(JSON.stringify({ success: true, data: { lines: ['log line 1'] } }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      }
      if (u.includes('/api/vkturn/config')) {
        return new Response(JSON.stringify({ success: true, data: { success: true } }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      }
      if (u.includes('/api/vkturn/action')) {
        return new Response(JSON.stringify({ success: true, data: { delay_ms: 35 } }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      }
      return new Response(JSON.stringify({ success: false }), { status: 404 })
    })

    const api = await import('./api')
    const { getVkTurnStatus, getVkTurnLogs, saveVkTurnConfig, vkTurnAction } = api
    // Конфиг туннеля выдаёт сервер, разбор вставленного конфига не нужен
    expect('parseVkTurnWg' in api).toBe(false)
    
    const status = await getVkTurnStatus()
    expect(status.phase).toBe('connected')

    const logs = await getVkTurnLogs()
    expect(logs.lines).toEqual(['log line 1'])

    const saved = await saveVkTurnConfig({ enabled: true, peer: '203.0.113.10', password: 'pw' })
    expect(saved.success).toBe(true)

    const action = await vkTurnAction('test')
    expect(action.delay_ms).toBe(35)

    fetchSpy.mockRestore()
  })
})
