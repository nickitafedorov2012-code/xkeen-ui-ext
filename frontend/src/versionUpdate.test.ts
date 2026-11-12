import { describe, it, expect, vi } from 'vitest'
// @ts-ignore
import { isUpdateAvailable } from './utils/version'
// @ts-ignore
import { useUpdateCheck } from './hooks/useUpdateCheck'
import * as api from './api'
import React, { act } from 'react'
import { createRoot } from 'react-dom/client'

describe('bug 140: update check & version comparison', () => {
  it('bug 140: single update poller and numeric version compare', async () => {
    // 1. Numeric version comparison oracle table
    expect(isUpdateAvailable('1.16.5', '1.16.5')).toBe(false)
    expect(isUpdateAvailable('1.16.4', '1.16.5')).toBe(true)
    expect(isUpdateAvailable('1.17.0-alpha', '1.16.5')).toBe(false)
    expect(isUpdateAvailable('invalid-ver', '1.16.5')).toBe(false)
    expect(isUpdateAvailable('1.16.5', 'invalid-ver')).toBe(false)
    expect(isUpdateAvailable('v1.19.0', 'v1.19.1')).toBe(true)
    expect(isUpdateAvailable('v1.19.1', 'v1.19.0')).toBe(false)
    expect(isUpdateAvailable('v1.19.0-rc1', 'v1.19.0')).toBe(false)
    expect(isUpdateAvailable('1.19.0.1', '1.19.0.2')).toBe(true)
    expect(isUpdateAvailable('1.19.0.2', '1.19.0.1')).toBe(false)

    // 2. Single poller test across multiple hook consumers
    vi.useFakeTimers()
    const apiGetSpy = vi.spyOn(api, 'apiGet').mockImplementation((path: string) => {
      if (path === 'update/check') {
        return Promise.resolve({
          current: '1.16.4',
          latest: '1.16.5',
          notes: ['Fix 1', 'Fix 2'],
          update_available: true,
        })
      }
      return Promise.resolve({})
    })

    let hook1Result: any = null
    let hook2Result: any = null

    function TestComponent1() {
      hook1Result = useUpdateCheck()
      return React.createElement('div', null, hook1Result?.updateInfo?.latest || '')
    }

    function TestComponent2() {
      hook2Result = useUpdateCheck()
      return React.createElement('div', null, hook2Result?.updateInfo?.latest || '')
    }

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(
        React.createElement('div', null,
          React.createElement(TestComponent1),
          React.createElement(TestComponent2)
        )
      )
    })

    // Let microtasks run
    await act(async () => {
      await Promise.resolve()
    })

    expect(apiGetSpy).toHaveBeenCalledTimes(1)
    expect(hook1Result?.updateInfo?.latest).toBe('1.16.5')
    expect(hook2Result?.updateInfo?.latest).toBe('1.16.5')

    // Advance 60s
    await act(async () => {
      vi.advanceTimersByTime(60_000)
      await Promise.resolve()
    })

    // Still only 1 call per tick across both consumers
    expect(apiGetSpy).toHaveBeenCalledTimes(2)

    await act(async () => {
      root.unmount()
    })
    container.remove()
    vi.useRealTimers()
    apiGetSpy.mockRestore()
  })
})
