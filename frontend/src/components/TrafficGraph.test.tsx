import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createRoot, Root } from 'react-dom/client'
import { act } from 'react'
import TrafficGraph from './TrafficGraph'
import * as api from '../api'

describe('TrafficGraph Component — Individual traffic resets & strictly 2 channels', () => {
  let container: HTMLDivElement | null = null
  let root: Root | null = null
  const notifyMock = vi.fn()

  beforeEach(() => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    vi.restoreAllMocks()
    notifyMock.mockClear()
  })

  afterEach(() => {
    if (root) {
      act(() => {
        root?.unmount()
      })
      root = null
    }
    if (container) {
      container.remove()
      container = null
    }
  })

  it('renders strictly two channels in VPS mode and allows resetting direct and vps traffic', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'traffic/poll') {
        return {
          direct: { down: 50000, up: 10000 },
          proxy: { down: 120000, up: 30000 },
          whitelist_active: false,
          direct_bytes: 1048576,
          proxy_bytes: 2097152,
          whitelist_bytes: 0,
        }
      }
      return {}
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation(async (path: string) => {
      if (path.includes('target=direct')) {
        return { status: 'ok', target: 'direct', direct_bytes: 0 }
      }
      if (path.includes('target=vps')) {
        return { status: 'ok', target: 'vps', proxy_bytes: 0 }
      }
      return { status: 'ok' }
    })

    await act(async () => {
      root!.render(<TrafficGraph notify={notifyMock} />)
    })

    // Strict two-channel rule: direct badge and vps badge must exist, whitelist must NOT exist
    const directBadge = container?.querySelector('.traffic-badge.direct') as HTMLElement
    const vpsBadge = container?.querySelector('.traffic-badge.proxy') as HTMLElement
    const wlBadge = container?.querySelector('.traffic-badge.whitelist') as HTMLElement

    expect(directBadge).toBeTruthy()
    expect(vpsBadge).toBeTruthy()
    expect(wlBadge).toBeNull()

    // Reset direct traffic
    const resetDirectBtn = container?.querySelector('button[aria-label="Сбросить накопленный прямой трафик"]') as HTMLButtonElement
    expect(resetDirectBtn).toBeTruthy()

    await act(async () => {
      resetDirectBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('traffic/reset?target=direct', {})
    expect(notifyMock).toHaveBeenCalledWith('Счётчик трафика (Прямой трафик) сброшен')

    // Reset vps traffic
    const resetVpsBtn = container?.querySelector('button[aria-label="Сбросить накопленный трафик VPS"]') as HTMLButtonElement
    expect(resetVpsBtn).toBeTruthy()

    await act(async () => {
      resetVpsBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('traffic/reset?target=vps', {})
    expect(notifyMock).toHaveBeenCalledWith('Счётчик трафика (VPS трафик) сброшен')
  })

  it('renders strictly two channels in Whitelist mode and allows resetting whitelist traffic', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    vi.spyOn(api, 'apiGet').mockImplementation(async (path: string) => {
      if (path === 'traffic/poll') {
        return {
          direct: { down: 50000, up: 10000 },
          whitelist: { down: 80000, up: 20000, active: true },
          whitelist_active: true,
          direct_bytes: 1048576,
          proxy_bytes: 2097152,
          whitelist_bytes: 5242880,
        }
      }
      return {}
    })
    const postSpy = vi.spyOn(api, 'apiPost').mockImplementation(async (path: string) => {
      if (path.includes('target=whitelist')) {
        return { status: 'ok', target: 'whitelist', whitelist_bytes: 0 }
      }
      return { status: 'ok' }
    })

    await act(async () => {
      root!.render(<TrafficGraph notify={notifyMock} />)
    })

    // Strict two-channel rule: direct badge and whitelist badge must exist, proxy badge must NOT exist
    const directBadge = container?.querySelector('.traffic-badge.direct') as HTMLElement
    const vpsBadge = container?.querySelector('.traffic-badge.proxy') as HTMLElement
    const wlBadge = container?.querySelector('.traffic-badge.whitelist') as HTMLElement

    expect(directBadge).toBeTruthy()
    expect(wlBadge).toBeTruthy()
    expect(vpsBadge).toBeNull()

    // Reset whitelist traffic
    const resetWlBtn = container?.querySelector('button[aria-label="Сбросить накопленный трафик БС"]') as HTMLButtonElement
    expect(resetWlBtn).toBeTruthy()

    await act(async () => {
      resetWlBtn.click()
    })

    expect(postSpy).toHaveBeenCalledWith('traffic/reset?target=whitelist', {})
    expect(notifyMock).toHaveBeenCalledWith('Счётчик трафика (Туннель БС) сброшен')
  })

  it('cancels reset if user rejects window.confirm', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false)
    vi.spyOn(api, 'apiGet').mockResolvedValue({
      direct: { down: 50000, up: 10000 },
      proxy: { down: 120000, up: 30000 },
      whitelist_active: false,
      direct_bytes: 1048576,
    })
    const postSpy = vi.spyOn(api, 'apiPost')

    await act(async () => {
      root!.render(<TrafficGraph notify={notifyMock} />)
    })

    const resetDirectBtn = container?.querySelector('button[aria-label="Сбросить накопленный прямой трафик"]') as HTMLButtonElement
    expect(resetDirectBtn).toBeTruthy()

    await act(async () => {
      resetDirectBtn.click()
    })

    expect(postSpy).not.toHaveBeenCalled()
    expect(notifyMock).not.toHaveBeenCalled()
  })
})
