import { describe, it, expect, vi, afterEach } from 'vitest'
import { apiGet, validateEndpointData } from './api'

describe('api.ts hardening', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('validates payloads for status, routing, zapret/status and adguard/status', () => {
    expect(() => validateEndpointData('status', { version: '1.16.10', available: false })).not.toThrow()
    expect(() => validateEndpointData('status', { bad: true })).toThrow()

    expect(() => validateEndpointData('routing', { mode: 'rule' })).not.toThrow()
    expect(() => validateEndpointData('routing', [])).toThrow()

    expect(() => validateEndpointData('zapret/status', { installed: true, running: false })).not.toThrow()
    expect(() => validateEndpointData('zapret/status', {})).toThrow()

    expect(() => validateEndpointData('adguard/status', { installed: true, running: true })).not.toThrow()
    expect(() => validateEndpointData('adguard/status', 'oops')).toThrow()
  })

  it('aborts request when caller AbortSignal is aborted without throwing timeout message', async () => {
    const controller = new AbortController()
    const fetchMock = vi.fn((_url: string, init?: RequestInit) => {
      return new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () => {
          reject(new DOMException('The operation was aborted', 'AbortError'))
        })
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const p = apiGet('/status', controller.signal)
    controller.abort()

    await expect(p).rejects.toMatchObject({ name: 'AbortError' })
  })
})
