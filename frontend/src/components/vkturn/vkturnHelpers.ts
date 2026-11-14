import type { VkTurnStatus, VkTurnCoreInstall } from '../../types'

export type VkTurnAuthMode = 'manual' | 'oauth' | 'profile_token'
export interface VkTurnProps {
  notify: (msg: string, error?: boolean) => void
}

/** Параметры официального приложения VK для звонков (CSQTT) */
export const VK_APP_ID = '7793118'
export const VK_OAUTH_SCOPE = '1073737727'
export const VK_REDIRECT_URI = 'https://oauth.vk.ru/blank.html'
export const VK_AUTH_URL =
  'https://oauth.vk.ru/authorize?client_id=7793118&scope=1073737727&redirect_uri=https%3A%2F%2Foauth.vk.ru%2Fblank.html&display=page&response_type=token&revoke=1&v=5.199'

/** Разбор redirect_uri fragment (#access_token=...&user_id=...) */
export function parseVkRedirectFragment(urlOrFragment: string): { token: string; userId?: number } | null {
  let input = urlOrFragment.trim()
  if (!input) return null
  try {
    input = decodeURIComponent(input)
  } catch {
    // Оставляем исходную строку, если URI-декодирование не удалось
  }

  let token = ''
  let userId: number | undefined

  const tokenMatch = input.match(/access_token=([a-zA-Z0-9._-]+)/)
  if (tokenMatch) {
    token = tokenMatch[1]
  }

  const userMatch = input.match(/user_id=([0-9]+)/)
  if (userMatch) {
    const parsed = parseInt(userMatch[1], 10)
    if (!isNaN(parsed)) userId = parsed
  }

  if (token) return { token, userId }
  if (input.startsWith('vk1.a.') || (!input.includes('=') && input.length > 20)) {
    return { token: input }
  }
  return null
}

/** PEER-порт сервера csqtt на VPS по умолчанию */
export const DEFAULT_CSQTT_PORT = 46000
/** Клиент csqtt использует до шести ссылок одновременно */
export const MAX_LINKS_FALLBACK = 6
export const DEFAULT_STREAMS = 20

/** Хост из "host:port" / "[v6]:port" для подстановки в поле адреса сервера. */
export function hostFromPeer(peer?: string | null): string {
  if (!peer) return ''
  const i = peer.lastIndexOf(':')
  const host = i > 0 ? peer.slice(0, i) : peer
  return host.replace(/^\[|\]$/g, '').trim()
}

/** Порт из "host:port" или значение по умолчанию. */
export function portFromPeer(peer?: string | null): string {
  if (!peer) return String(DEFAULT_CSQTT_PORT)
  const i = peer.lastIndexOf(':')
  if (i > 0) {
    const candidate = peer.slice(i + 1).trim()
    if (/^\d+$/.test(candidate)) return candidate
  }
  return String(DEFAULT_CSQTT_PORT)
}

export function formatTraffic(bytes?: number | null): string {
  const b = Number(bytes) || 0
  if (b < 1024) return `${b} Б`
  if (b < 1024 ** 2) return `${(b / 1024).toFixed(1)} КБ`
  if (b < 1024 ** 3) return `${(b / 1024 ** 2).toFixed(1)} МБ`
  return `${(b / 1024 ** 3).toFixed(2)} ГБ`
}

/** Сколько скачано из компонента: «3.2 МБ из 10.3 МБ (31%)» */
export function installProgressText(install?: VkTurnCoreInstall | null): string {
  const done = Number(install?.downloaded) || 0
  const total = Number(install?.total) || 0
  if (total > 0) {
    const pct = Math.min(100, Math.floor((done / total) * 100))
    return `${formatTraffic(done)} из ${formatTraffic(total)} (${pct}%)`
  }
  return done > 0 ? formatTraffic(done) : 'подключаюсь к серверу загрузки'
}

/**
 * Компонент скачивается на роутере в фоне — на медленном канале это минуты, дольше таймаута
 * одного запроса панели. Ждём окончания по статусу.
 */
export async function waitForCoreInstall(
  fetchStatus: () => Promise<VkTurnStatus>,
  onStatus: (s: VkTurnStatus) => void,
  pollMs = 1500,
  maxWaitMs = 16 * 60 * 1000,
): Promise<void> {
  const deadline = Date.now() + maxWaitMs
  while (Date.now() < deadline) {
    if (pollMs > 0) await new Promise((r) => setTimeout(r, pollMs))
    let st: VkTurnStatus
    try {
      st = await fetchStatus()
    } catch {
      continue // роутер мог на секунду не ответить — продолжаем ждать
    }
    onStatus(st)
    if (st.install?.running) continue
    if (st.install?.error) throw new Error(`Не удалось скачать компонент: ${st.install.error}`)
    if (st.installed) return
    throw new Error('Компонент не установился — попробуйте ещё раз')
  }
  throw new Error('Компонент скачивается слишком долго — проверьте интернет на роутере и попробуйте ещё раз')
}

/** Интервал опроса статуса во время установки компонента (в тестах уменьшается). */
export const vkTurnTiming = { installPollMs: 1500 }

export const badgeStyle = { fontSize: 13, padding: '4px 10px' }

export function errMessage(err: unknown, fallback: string): string {
  return err instanceof Error && err.message ? err.message : fallback
}

export function resolveEffectiveAuthMode(status: VkTurnStatus | null): VkTurnAuthMode {
  if (status?.auth_mode === 'profile_token' && status?.has_profile_token) return 'profile_token'
  if (status?.auth_mode === 'manual') return 'manual'
  if (status?.auth_mode === 'oauth') return 'oauth'
  if (status?.has_profile_token) return 'profile_token'
  if (status?.has_token) return 'oauth'
  return 'manual'
}

