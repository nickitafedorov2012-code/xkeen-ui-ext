import { useState, useEffect, useRef, useCallback } from 'react'
import {
  getVkTurnStatus,
  getVkTurnLogs,
  saveVkTurnConfig,
  vkTurnAction,
  saveVkTurnAuth,
  logoutVkTurnAuth,
  decryptCsqttProfile,
} from '../api'
import type { VkTurnStatus, VkTurnPhase, VkTurnCoreInstall } from '../types'
import { parseCsqttLink } from '../utils/csqttLink'

type VkTurnAuthMode = 'manual' | 'oauth' | 'profile_token'

interface VkTurnProps {
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
const MAX_LINKS_FALLBACK = 6
const DEFAULT_STREAMS = 20

/** Хост из "host:port" / "[v6]:port" для подстановки в поле адреса сервера. */
export function hostFromPeer(peer?: string | null): string {
  if (!peer) return ''
  const i = peer.lastIndexOf(':')
  const host = i > 0 ? peer.slice(0, i) : peer
  return host.replace(/^\[|\]$/g, '').trim()
}

/** Порт из "host:port" или значение по умолчанию. */
function portFromPeer(peer?: string | null): string {
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

const badgeStyle = { fontSize: 13, padding: '4px 10px' }

function errMessage(err: unknown, fallback: string): string {
  return err instanceof Error && err.message ? err.message : fallback
}

function resolveEffectiveAuthMode(status: VkTurnStatus | null): VkTurnAuthMode {
  if (status?.auth_mode === 'profile_token' && status?.has_profile_token) return 'profile_token'
  if (status?.auth_mode === 'manual') return 'manual'
  if (status?.auth_mode === 'oauth') return 'oauth'
  if (status?.has_profile_token) return 'profile_token'
  if (status?.has_token) return 'oauth'
  return 'manual'
}

export default function VkTurn({ notify }: VkTurnProps) {
  const [status, setStatus] = useState<VkTurnStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [statusUnavailable, setStatusUnavailable] = useState(false)
  const [actionPending, setActionPending] = useState(false)
  const [logs, setLogs] = useState<string[]>([])
  const [loadingLogs, setLoadingLogs] = useState(false)
  const [accordionOpen, setAccordionOpen] = useState(false)
  const [testResult, setTestResult] = useState<{ delay: number | null; at: Date } | null>(null)
  const [testingPing, setTestingPing] = useState(false)
  const testTicket = useRef(0)

  // Мастер настройки (строковый ввод порта сохраняет свободный Backspace)
  const [peerHost, setPeerHost] = useState('')
  const [portInput, setPortInput] = useState<string>(String(DEFAULT_CSQTT_PORT))
  const [password, setPassword] = useState('')
  const [callLinks, setCallLinks] = useState<string[]>([''])
  const [sourceMode, setSourceMode] = useState<VkTurnAuthMode | null>(null)
  const [profileToken, setProfileToken] = useState<string | null>(null)
  const [profilePower, setProfilePower] = useState<number | null>(null)
  const wizardTouchedRef = useRef(false)

  // Карточка мониторинга: редактирование адреса сервера, ссылки, пароля и потоков
  const [newLinkInput, setNewLinkInput] = useState('')
  const [updatingLink, setUpdatingLink] = useState(false)
  const [newPassword, setNewPassword] = useState('')
  const [updatingPassword, setUpdatingPassword] = useState(false)
  const [editPeerInput, setEditPeerInput] = useState('')
  const [updatingPeer, setUpdatingPeer] = useState(false)
  const peerEditedRef = useRef(false)
  const [manualSwitchPending, setManualSwitchPending] = useState(false)

  const [draftStreamsInput, setDraftStreamsInput] = useState<string | null>(null)
  const [streamsUpdating, setStreamsUpdating] = useState(false)

  // VK OAuth модальное окно
  const [authModalOpen, setAuthModalOpen] = useState(false)
  const [manualTokenInput, setManualTokenInput] = useState('')
  const [authSubmitting, setAuthSubmitting] = useState(false)

  const pollingRef = useRef<boolean>(false)
  const importTicket = useRef(0)

  useEffect(() => () => { importTicket.current += 1 }, [])

  useEffect(() => {
    testTicket.current += 1
    setTestResult(null)
  }, [status?.peer, status?.pid, status?.restarts, status?.enabled, status?.tunnel_ready])

  // Синхронизация редактируемого адреса сервера и полей мастера при возврате из сохранённого состояния
  useEffect(() => {
    if (!status) return
    if (status.peer && !peerEditedRef.current) {
      setEditPeerInput(status.peer)
    }
    if (!wizardTouchedRef.current) {
      if (status.peer && !peerHost) {
        setPeerHost(hostFromPeer(status.peer))
        setPortInput(portFromPeer(status.peer))
      }
      if (status.links && status.links.length > 0 && callLinks.length === 1 && callLinks[0] === '') {
        setCallLinks(status.links)
      }
    }
    if (status.auth_mode === 'manual' && manualSwitchPending) {
      setManualSwitchPending(false)
    }
  }, [status, peerHost, callLinks, manualSwitchPending])

  const selectedSource: VkTurnAuthMode =
    sourceMode ??
    (status?.auth_mode === 'profile_token' && status.has_profile_token
      ? 'profile_token'
      : status?.auth_mode === 'manual' && (status?.links_count ?? 0) > 0
        ? 'manual'
        : status?.has_token
          ? 'oauth'
          : 'manual')

  const loadStatus = useCallback(async (isInitial = false) => {
    if (document.hidden && !isInitial) return
    if (pollingRef.current) return
    pollingRef.current = true
    try {
      const data = await getVkTurnStatus()
      setStatus(data)
      setStatusUnavailable(false)
    } catch {
      setStatusUnavailable(true)
    } finally {
      pollingRef.current = false
      if (isInitial) setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadStatus(true)
    const interval = setInterval(() => {
      loadStatus(false)
    }, 3000)
    return () => clearInterval(interval)
  }, [loadStatus])

  const fetchLogs = async () => {
    setLoadingLogs(true)
    try {
      const res = await getVkTurnLogs()
      setLogs(res.lines || [])
    } catch (err: unknown) {
      notify(errMessage(err, 'Не удалось загрузить логи VK-туннеля'), true)
    } finally {
      setLoadingLogs(false)
    }
  }

  const maxLinks = status?.max_links || MAX_LINKS_FALLBACK

  const installCore = async () => {
    await vkTurnAction('install_core')
    await waitForCoreInstall(getVkTurnStatus, setStatus, vkTurnTiming.installPollMs)
  }

  const handleCallLinkChange = (index: number, val: string) => {
    wizardTouchedRef.current = true
    setCallLinks((prev) => {
      const copy = [...prev]
      copy[index] = val
      return copy
    })
  }

  // Мастер: «Включить»
  const handleWizardEnable = async () => {
    const host = peerHost.trim()
    if (!host) {
      notify('Укажите адрес сервера VPS (IP или домен)', true)
      return
    }
    const rawPort = portInput.trim()
    const portNum = rawPort === '' ? DEFAULT_CSQTT_PORT : Number(rawPort)
    if (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535) {
      notify('Порт сервера csqtt должен быть от 1 до 65535', true)
      return
    }
    const hasSavedPassword = Boolean(status?.has_password)
    if (!password.trim() && !hasSavedPassword) {
      notify('Укажите пароль подключения к серверу csqtt', true)
      return
    }
    const links = callLinks.map((l) => l.trim()).filter(Boolean)
    if (
      (selectedSource === 'manual' && links.length === 0) ||
      (selectedSource === 'oauth' && !status?.has_token) ||
      (selectedSource === 'profile_token' && !(profileToken || (profileToken === null && status?.has_profile_token)))
    ) {
      notify('Укажите хотя бы одну ссылку на звонок или выполните вход через VK ID', true)
      return
    }

    setActionPending(true)
    try {
      if (status && !status.installed) {
        notify('Скачиваю компонент VK-туннеля (около 11 МБ) — на медленном интернете это займёт несколько минут…')
        await installCore()
      }

      notify('Сохранение конфигурации и запуск...')
      const peer = host.includes(':') ? `[${host}]:${portNum}` : `${host}:${portNum}`
      await saveVkTurnConfig({
        enabled: true,
        peer,
        ...(password.trim() ? { password: password.trim() } : {}),
        links: selectedSource === 'manual' ? links : [],
        auth_mode: selectedSource,
        ...(profileToken !== null ? { profile_token: profileToken } : {}),
        ...(profilePower !== null ? { streams: profilePower } : {}),
      })
      setPassword('')
      wizardTouchedRef.current = false
      notify('VK-туннель настроен. Соединение устанавливается автоматически.')
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка настройки VK-туннеля'), true)
    } finally {
      setActionPending(false)
    }
  }

  const resolveCsqttInput = async (raw: string) => {
    const trimmed = raw.trim()
    let parsed = parseCsqttLink(trimmed)
    if (!parsed && /^csqtt:\/\/[a-z0-9_+/=-]{40,}$/i.test(trimmed)) {
      const result = await decryptCsqttProfile(trimmed)
      parsed = parseCsqttLink(result.link)
    }
    return parsed
  }

  const importProfile = async (raw: string): Promise<boolean> => {
    const ticket = ++importTicket.current
    const trimmed = raw.trim()
    let parsed = parseCsqttLink(trimmed)
    if (!parsed && /^csqtt:\/\/[a-z0-9_+/=-]{40,}$/i.test(trimmed)) {
      try {
        const result = await decryptCsqttProfile(trimmed)
        parsed = parseCsqttLink(result.link)
      } catch {
        if (ticket === importTicket.current) notify('Не удалось прочитать профиль CSQTTR', true)
        return false
      }
    }
    if (ticket !== importTicket.current || !parsed) return false
    if (parsed.host.includes(':')) {
      notify('Адреса IPv6 в профилях пока не поддерживаются; нужен IPv4 или домен', true)
      return false
    }
    wizardTouchedRef.current = true
    setPeerHost(parsed.host)
    setPortInput(String(parsed.port))
    setPassword(parsed.password)
    setCallLinks(parsed.hashes?.map((h) => `https://vk.com/call/join/${h}`) || [''])
    setProfileToken(parsed.token || '')
    setProfilePower(parsed.power ?? null)
    setSourceMode(
      parsed.token ? 'profile_token' : parsed.hashes?.length ? 'manual' : (status?.has_token ? 'oauth' : 'manual'),
    )
    notify('Настройки сервера, порт и пароль успешно извлечены из ссылки csqtt://')
    return true
  }

  const handleHostChange = (val: string) => {
    wizardTouchedRef.current = true
    setPeerHost(val)
    void importProfile(val)
  }

  const handleImportFromClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText()
      if (!await importProfile(text)) {
        notify('В буфере обмена нет ссылки csqtt:// (ожидается csqtt://connect?v=2... или csqtt://password@host:port)', true)
      }
    } catch {
      notify('Не удалось прочитать буфер обмена. Вставьте ссылку вручную в поле адреса', true)
    }
  }

  // Запуск OAuth авторизации VK в popup
  const handleStartVkOAuth = () => {
    const width = 640
    const height = 660
    const left = Math.max(0, Math.floor(window.screenX + (window.outerWidth - width) / 2))
    const top = Math.max(0, Math.floor(window.screenY + (window.outerHeight - height) / 2))
    const popup = window.open(
      VK_AUTH_URL,
      'vk_oauth_popup',
      `width=${width},height=${height},left=${left},top=${top},status=no,toolbar=no,menubar=no`,
    )

    setAuthModalOpen(true)

    if (!popup) {
      notify('Всплывающее окно заблокировано браузером. Вставьте адрес авторизации вручную', true)
    }
  }

  const handlePasteFromClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText()
      const trimmed = text.trim()
      if (!trimmed) {
        notify('Буфер обмена пуст — скопируйте адрес из адресной строки окна VK', true)
        return
      }
      await handleSaveAuthString(trimmed)
    } catch {
      notify('Нажмите сочетание Ctrl+V в поле ввода ниже, чтобы вставить адрес', true)
    }
  }

  const hasActiveAuthSource =
    status?.auth_mode === 'manual'
      ? (status?.links_count ?? 0) > 0
      : status?.auth_mode === 'profile_token'
        ? Boolean(status?.has_profile_token)
        : Boolean(status?.has_token) || (status?.links_count ?? 0) > 0
  const isConfigured = Boolean(status?.peer && status?.has_password && hasActiveAuthSource)

  const handleSaveAuthString = async (str: string) => {
    const trimmed = str.trim()
    if (!trimmed) {
      notify('Введите ссылку авторизации или access_token', true)
      return
    }
    setAuthSubmitting(true)
    try {
      const parsed = parseVkRedirectFragment(trimmed)
      const token = parsed?.token || trimmed
      const res = await saveVkTurnAuth({
        token,
        user_id: parsed?.userId,
        url: trimmed,
        ...(!isConfigured ? { auth_mode: 'oauth' } : {}),
      })
      if (res.success) {
        notify(res.message || `✓ Учетная запись ${res.user_name || ''} успешно подключена!`)
        setAuthModalOpen(false)
        setManualTokenInput('')
        setSourceMode('oauth')
        await loadStatus(true)
      } else {
        notify('Ошибка сохранения данных авторизации VK', true)
      }
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка авторизации VK'), true)
    } finally {
      setAuthSubmitting(false)
    }
  }

  const handleLogoutVk = async () => {
    if (!window.confirm('Вы действительно хотите выйти из учетной записи VK?')) return
    setActionPending(true)
    try {
      const res = await logoutVkTurnAuth()
      if (res.success) {
        notify('Вы вышли из учетной записи VK')
        await loadStatus(true)
      }
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка выхода из учетной записи VK'), true)
    } finally {
      setActionPending(false)
    }
  }

  const handleToggleEnabled = async (checked: boolean) => {
    setActionPending(true)
    try {
      await saveVkTurnConfig({ enabled: checked })
      notify(checked ? 'VK-туннель включен' : 'VK-туннель выключен')
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка переключения туннеля'), true)
    } finally {
      setActionPending(false)
    }
  }

  const handleSwitchAuthMode = async (mode: VkTurnAuthMode) => {
    if (mode === 'oauth' && !status?.has_token) {
      handleStartVkOAuth()
      return
    }
    if (mode === 'manual' && (status?.links_count ?? 0) === 0) {
      setManualSwitchPending(true)
      setAccordionOpen(true)
      notify('Добавьте ссылку на звонок ВКонтакте для переключения на ручной режим')
      return
    }
    setActionPending(true)
    try {
      await saveVkTurnConfig({ auth_mode: mode })
      setManualSwitchPending(false)
      const label =
        mode === 'oauth' ? 'Мой аккаунт VK' : mode === 'profile_token' ? 'Токен профиля' : 'Ручные ссылки'
      notify(`Источник звонков переключён: ${label}`)
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Не удалось переключить источник звонков'), true)
    } finally {
      setActionPending(false)
    }
  }

  const handleTest = async () => {
    const ticket = ++testTicket.current
    setTestResult(null)
    setTestingPing(true)
    try {
      const res = await vkTurnAction('test')
      if (ticket !== testTicket.current) return
      if (typeof res.delay_ms === 'number' && res.delay_ms > 0) {
        setTestResult({ delay: res.delay_ms, at: new Date() })
        notify(`Проверка связи: задержка ${res.delay_ms} мс`)
      } else {
        setTestResult({ delay: null, at: new Date() })
        notify('Не удалось измерить задержку туннеля', true)
      }
    } catch (err: unknown) {
      if (ticket === testTicket.current) {
        setTestResult({ delay: null, at: new Date() })
        notify(errMessage(err, 'Ошибка проверки туннеля'), true)
      }
    } finally {
      setTestingPing(false)
    }
  }

  const handleRestart = async () => {
    testTicket.current += 1
    setTestResult(null)
    setActionPending(true)
    try {
      await vkTurnAction('restart')
      notify('VK-туннель перезапущен')
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка перезапуска туннеля'), true)
    } finally {
      setActionPending(false)
    }
  }

  const handleResetServer = async () => {
    if (!window.confirm('Сбросить текущий сервер csqtt и настройки туннеля? Вы вернетесь в мастер настройки для ввода нового сервера.')) {
      return
    }
    testTicket.current += 1
    setTestResult(null)
    setActionPending(true)
    try {
      await vkTurnAction('reset')
      wizardTouchedRef.current = false
      peerEditedRef.current = false
      setPeerHost('')
      setPortInput(String(DEFAULT_CSQTT_PORT))
      setPassword('')
      setCallLinks([''])
      setSourceMode(null)
      setProfileToken(null)
      setProfilePower(null)
      setLogs([])
      setNewPassword('')
      setNewLinkInput('')
      setEditPeerInput('')
      setManualSwitchPending(false)
      notify('Настройки сервера csqtt сброшены. Вы можете указать новый сервер.')
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка сброса настроек туннеля'), true)
    } finally {
      setActionPending(false)
    }
  }

  const handleInstallCore = async () => {
    setActionPending(true)
    try {
      notify('Скачиваю компонент VK-туннеля (около 11 МБ) — на медленном интернете это займёт несколько минут…')
      await installCore()
      notify('Компонент успешно установлен')
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка установки компонента'), true)
    } finally {
      setActionPending(false)
    }
  }

  const handleUpdateCallLink = async (asBackup = false) => {
    const trimmed = newLinkInput.trim()
    if (!trimmed) {
      notify('Введите новую ссылку на звонок', true)
      return
    }
    setUpdatingLink(true)
    try {
      await saveVkTurnConfig(
        asBackup
          ? { append_link: trimmed, auth_mode: 'manual' }
          : { primary_link: trimmed, auth_mode: 'manual' },
      )
      notify(asBackup ? 'Запасная ссылка добавлена' : 'Ссылка на звонок обновлена')
      setNewLinkInput('')
      setManualSwitchPending(false)
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка обновления ссылки'), true)
    } finally {
      setUpdatingLink(false)
    }
  }

  const handleRemoveLink = async (index: number) => {
    setUpdatingLink(true)
    try {
      await saveVkTurnConfig({ remove_link_index: index })
      notify('Ссылка на звонок удалена')
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Не удалось удалить ссылку'), true)
    } finally {
      setUpdatingLink(false)
    }
  }

  const handleUpdatePassword = async () => {
    const trimmed = newPassword.trim()
    if (!trimmed) {
      notify('Введите новый пароль подключения', true)
      return
    }
    setUpdatingPassword(true)
    try {
      await saveVkTurnConfig({ password: trimmed })
      notify('Пароль подключения обновлён, переподключаюсь')
      setNewPassword('')
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка обновления пароля'), true)
    } finally {
      setUpdatingPassword(false)
    }
  }

  const handleUpdatePeer = async () => {
    const trimmed = editPeerInput.trim()
    if (!trimmed) {
      notify('Укажите адрес сервера (host:port или ссылку csqtt://)', true)
      return
    }
    setUpdatingPeer(true)
    try {
      if (trimmed.toLowerCase().startsWith('csqtt://')) {
        const parsed = await resolveCsqttInput(trimmed)
        if (!parsed) {
          notify('Не удалось разобрать ссылку csqtt://', true)
          return
        }
        const nextPeer = parsed.host.includes(':') ? `[${parsed.host}]:${parsed.port}` : `${parsed.host}:${parsed.port}`
        await saveVkTurnConfig({
          peer: nextPeer,
          ...(parsed.password ? { password: parsed.password } : {}),
          ...(parsed.token ? { profile_token: parsed.token, auth_mode: 'profile_token' } : {}),
          ...(parsed.power ? { streams: parsed.power } : {}),
        })
        setEditPeerInput(nextPeer)
        peerEditedRef.current = false
        notify('Адрес и параметры профиля csqtt:// обновлены')
      } else {
        const normalizedPeer = trimmed.includes(':') ? trimmed : `${trimmed}:${DEFAULT_CSQTT_PORT}`
        await saveVkTurnConfig({ peer: normalizedPeer })
        setEditPeerInput(normalizedPeer)
        peerEditedRef.current = false
        notify('Адрес сервера csqtt обновлён, переподключаюсь')
      }
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Ошибка обновления адреса сервера'), true)
    } finally {
      setUpdatingPeer(false)
    }
  }

  const handleSaveStreams = async () => {
    const candidate =
      draftStreamsInput !== null && /^\d+$/.test(draftStreamsInput.trim())
        ? Number(draftStreamsInput.trim())
        : (status?.target_streams ?? DEFAULT_STREAMS)
    if (!Number.isInteger(candidate) || candidate < 3 || candidate > 100) {
      notify('Число потоков должно быть от 3 до 100', true)
      return
    }
    setStreamsUpdating(true)
    try {
      await saveVkTurnConfig({ streams: candidate })
      notify(`Число потоков изменено на ${candidate}; туннель переподключается`)
      setDraftStreamsInput(null)
      await loadStatus(true)
    } catch (err: unknown) {
      notify(errMessage(err, 'Не удалось изменить число потоков'), true)
      await loadStatus(true)
    } finally {
      setStreamsUpdating(false)
    }
  }

  if (loading && !status) {
    return (
      <div className="vk-section" style={{ padding: 40, textAlign: 'center' }}>
        <div className="spinner" style={{ margin: '0 auto 16px' }} />
        <div className="muted">Загрузка модуля VK-туннель…</div>
      </div>
    )
  }

  if (statusUnavailable || !status) {
    return (
      <section className="vk-section" role="alert">
        <h2>Не удалось получить состояние VK-туннеля</h2>
        <p className="muted">Состояние туннеля сейчас неизвестно. Повторите попытку после восстановления связи.</p>
        <button type="button" className="btn" onClick={() => void loadStatus(true)}>Повторить</button>
      </section>
    )
  }

  const installing = Boolean(status?.install?.running)
  const effectiveAuthMode = resolveEffectiveAuthMode(status)

  const renderPhaseBadge = (phase: VkTurnPhase = 'disabled', connected = 0) => {
    switch (phase) {
      case 'connected':
        if (!status?.tunnel_ready || connected === 0) {
          return <span className="badge badge-warning" data-testid="status-badge" style={badgeStyle}>🟡 Подключаю туннель…</span>
        }
        return (
          <span className="badge badge-success" data-testid="status-badge" style={badgeStyle}>
            🟢 Подключён · соединений: {connected}
          </span>
        )
      case 'starting':
      case 'authorizing':
        return (
          <span className="badge badge-warning" data-testid="status-badge" style={badgeStyle}>
            🟡 Подключаюсь…
          </span>
        )
      case 'solving_captcha':
        return (
          <span className="badge badge-warning" data-testid="status-badge" style={badgeStyle}>
            🟡 ВК проверяет, что мы не робот…
          </span>
        )
      case 'captcha_required':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 ВК запросил капчу, повторю через 10 минут
          </span>
        )
      case 'link_invalid':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 Ссылка на звонок не работает
          </span>
        )
      case 'auth_failed':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            {status?.error_source === 'vk' ? '🔴 Ошибка авторизации VK' : '🔴 Сервер отклонил пароль'}
          </span>
        )
      case 'server_unreachable':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 Сервер на VPS не отвечает
          </span>
        )
      case 'degraded':
      case 'backoff':
        return (
          <span className="badge badge-warning" data-testid="status-badge" style={badgeStyle}>
            🟡 Переподключаюсь
          </span>
        )
      case 'not_installed':
        return (
          <span className="badge badge-neutral" data-testid="status-badge" style={badgeStyle}>
            Компонент не установлен
          </span>
        )
      case 'crashed':
        return (
          <span className="badge badge-danger" data-testid="status-badge" style={badgeStyle}>
            🔴 Сбой процесса
          </span>
        )
      case 'disabled':
      default:
        return (
          <span className="badge badge-neutral" data-testid="status-badge" style={badgeStyle}>
            Отключен
          </span>
        )
    }
  }

  const renderVkAccountCard = () => {
    const isAuthorized = Boolean(status?.has_token || status?.vk_user_id)
    const isProfileTokenActive = effectiveAuthMode === 'profile_token' && Boolean(status?.has_profile_token)

    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {isProfileTokenActive && (
          <div
            className="vk-subcard vk-profile-token-card"
            data-testid="vk-profile-token-card"
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              flexWrap: 'wrap',
              gap: 10,
              borderColor: 'rgba(34, 197, 94, 0.35)',
            }}
          >
            <div>
              <div style={{ fontWeight: 600, fontSize: 14 }}>🔑 Токен из профиля csqtt:// активен</div>
              <div className="muted" style={{ fontSize: 12, marginTop: 3 }}>
                Звонки создаются автоматически по токену, встроенному в подключённый профиль сервера.
              </div>
            </div>
            <span className="badge badge-success" style={{ fontSize: 12 }}>Готов</span>
          </div>
        )}

        {isAuthorized ? (
          <div
            className="vk-subcard vk-account-card"
            data-testid="vk-account-card"
            style={{
              borderColor: 'rgba(34, 197, 94, 0.35)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              flexWrap: 'wrap',
              gap: 12,
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
              <div
                style={{
                  width: 38,
                  height: 38,
                  borderRadius: '50%',
                  background: '#0077ff',
                  color: '#fff',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  fontWeight: 700,
                  fontSize: 16,
                  flexShrink: 0,
                }}
              >
                VK
              </div>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                  <span className={status?.vk_token_verified === false ? 'badge badge-warning' : 'badge badge-success'} style={{ fontSize: 12 }}>
                    {status?.vk_token_verified === false ? 'Токен сохранён · аккаунт не проверен' : '✓ Учетная запись подключена'}
                  </span>
                  <span style={{ fontWeight: 600, fontSize: 14 }}>
                    {status?.vk_user_name || 'Пользователь VK'}
                  </span>
                  {status?.vk_user_id && (
                    <span className="badge badge-neutral" style={{ fontSize: 11 }}>
                      ID: {status.vk_user_id}
                    </span>
                  )}
                </div>
                <div className="muted" style={{ fontSize: 12, marginTop: 4 }}>
                  {isConfigured && effectiveAuthMode !== 'oauth'
                    ? 'Для туннеля сейчас выбран другой источник звонков'
                    : 'Аккаунт доступен для автоматического создания звонков'}
                </div>
              </div>
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              {isConfigured && effectiveAuthMode !== 'oauth' && (
                <button
                  type="button"
                  className="btn btn-primary"
                  data-testid="use-vk-account-btn"
                  disabled={actionPending}
                  onClick={() => void handleSwitchAuthMode('oauth')}
                  style={{ fontSize: 13, padding: '6px 14px' }}
                >
                  Использовать этот аккаунт
                </button>
              )}
              <button
                type="button"
                className="btn btn-secondary"
                disabled={actionPending}
                onClick={handleLogoutVk}
                style={{ fontSize: 13, padding: '6px 14px' }}
              >
                Выйти из аккаунта
              </button>
            </div>
          </div>
        ) : (
          <div
            className="vk-auth-banner vk-auth-card"
            data-testid="vk-auth-card"
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              flexWrap: 'wrap',
              gap: 12,
            }}
          >
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                <span style={{ fontSize: 20 }}>🔵</span>
                <h4 style={{ margin: 0, fontSize: 15, fontWeight: 700 }}>Учетная запись ВКонтакте (VK ID)</h4>
                {!isProfileTokenActive && <span className="badge badge-warning" style={{ fontSize: 11 }}>Рекомендуется</span>}
              </div>
              <div className="muted" style={{ fontSize: 13 }}>
                Войдите через VK ID: роутер будет сам создавать звонки и удерживать их без ручного копирования ссылок.
              </div>
            </div>
            <button
              type="button"
              className="btn"
              data-testid="vk-login-btn"
              onClick={handleStartVkOAuth}
              style={{
                background: '#0077ff',
                color: '#ffffff',
                border: 'none',
                fontWeight: 600,
                fontSize: 14,
                padding: '9px 18px',
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
                boxShadow: '0 2px 8px rgba(0, 119, 255, 0.35)',
                cursor: 'pointer',
                borderRadius: 8,
              }}
            >
              <span>🔵</span>
              <span>Войти через ВКонтакте (VK ID)</span>
            </button>
          </div>
        )}
      </div>
    )
  }

  const renderAuthModal = () => {
    if (!authModalOpen) return null
    return (
      <div
        data-testid="auth-modal"
        style={{
          position: 'fixed',
          top: 0,
          left: 0,
          right: 0,
          bottom: 0,
          background: 'rgba(0, 0, 0, 0.72)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          zIndex: 9999,
          padding: 16,
        }}
      >
        <div
          className="vk-section"
          style={{
            maxWidth: 580,
            width: '100%',
            padding: 24,
            borderRadius: 16,
            boxShadow: '0 8px 32px rgba(0, 0, 0, 0.45)',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ fontSize: 24 }}>🔵</span>
              <h3 style={{ margin: 0, fontSize: 17, fontWeight: 700 }}>Подключение аккаунта ВКонтакте</h3>
            </div>
            <button
              type="button"
              onClick={() => setAuthModalOpen(false)}
              style={{ background: 'none', border: 'none', fontSize: 20, color: 'var(--muted)', cursor: 'pointer' }}
            >
              ✕
            </button>
          </div>

          <div className="vk-auth-banner" style={{ marginBottom: 16, fontSize: 13, lineHeight: 1.5 }}>
            <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--accent)' }}>💡 Как завершить вход:</div>
            <div>1. В открывшемся окне авторизуйтесь и подтвердите вход.</div>
            <div>2. Страница перейдёт на белый экран <code>oauth.vk.ru/blank.html#access_token=...</code></div>
            <div>3. Скопируйте адрес из адресной строки того окна (Ctrl+L, Ctrl+C). Вставьте его в поле ниже или используйте кнопку буфера.</div>
          </div>

          <div style={{ display: 'flex', gap: 10, marginBottom: 16 }}>
            <button
              type="button"
              className="btn"
              data-testid="modal-paste-btn"
              onClick={handlePasteFromClipboard}
              style={{
                flex: 1,
                background: '#0077ff',
                color: '#fff',
                padding: '10px 18px',
                fontWeight: 600,
                fontSize: 14,
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: 8,
                borderRadius: 8,
                border: 'none',
                cursor: 'pointer',
              }}
            >
              <span>📋</span>
              <span>Вставить из буфера и подключить</span>
            </button>
          </div>

          <div className="muted" style={{ fontSize: 12.5, marginBottom: 6 }}>
            Или вставьте скопированную ссылку / токен вручную:
          </div>

          <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 16 }}>
            <input
              type="text"
              className="input"
              autoFocus
              data-testid="modal-token-input"
              placeholder="https://oauth.vk.ru/blank.html#access_token=vk1.a...."
              value={manualTokenInput}
              onChange={(e) => setManualTokenInput(e.target.value)}
              style={{ flex: 1, fontSize: 13 }}
            />
            <button
              type="button"
              className="btn btn-primary"
              data-testid="modal-save-btn"
              disabled={authSubmitting || !manualTokenInput.trim()}
              onClick={() => handleSaveAuthString(manualTokenInput)}
              style={{ padding: '8px 16px', fontSize: 13 }}
            >
              {authSubmitting ? 'Сохранение…' : 'Подключить'}
            </button>
          </div>

          <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setAuthModalOpen(false)}
              style={{ fontSize: 13 }}
            >
              Закрыть
            </button>
          </div>
        </div>
      </div>
    )
  }

  // ---------------------------------------------------------
  // A. Мастер (туннель ещё не настроен)
  // ---------------------------------------------------------
  if (!isConfigured) {
    return (
      <div className="vkturn-container" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        <section className="vk-section" style={{ padding: '20px 24px' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16, flexWrap: 'wrap', marginBottom: 18 }}>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
                <h2 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Мастер настройки VK-туннеля</h2>
                <span className="badge badge-warning" style={{ fontSize: 12 }}>Первоначальная настройка</span>
              </div>
              <p className="muted" style={{ margin: 0, fontSize: 13.5 }}>
                Трафик идёт через серверы звонков ВКонтакте до вашего сервера csqtt на VPS — работает даже при белых списках.
              </p>
            </div>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={handleImportFromClipboard}
              style={{ fontSize: 12.5, padding: '6px 12px', display: 'inline-flex', alignItems: 'center', gap: 6 }}
              title="Импортировать ссылку csqtt:// из буфера обмена"
            >
              📋 Вставить ссылку csqtt://
            </button>
          </div>

          <h3 style={{ margin: '0 0 12px 0', fontSize: 15 }}>1️⃣ Сервер csqtt на VPS</h3>
          <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap', marginBottom: 16 }}>
            <div style={{ flex: '1 1 260px', maxWidth: 420 }}>
              <label style={{ display: 'block', fontSize: 13, marginBottom: 6, fontWeight: 600 }}>
                Адрес сервера (IP, домен или ссылка csqtt://):
              </label>
              <input
                type="text"
                className="input"
                placeholder="203.0.113.10 или csqtt://connect?..."
                value={peerHost}
                onChange={(e) => handleHostChange(e.target.value)}
                style={{ width: '100%' }}
              />
            </div>
            <div style={{ flex: '0 1 160px' }}>
              <label style={{ display: 'block', fontSize: 13, marginBottom: 6, fontWeight: 600 }}>
                Порт:
              </label>
              <input
                type="number"
                className="input"
                data-testid="vk-port-input"
                aria-label="Порт сервера csqtt"
                min={1}
                max={65535}
                value={portInput}
                onChange={(e) => {
                  wizardTouchedRef.current = true
                  setPortInput(e.target.value)
                }}
                style={{ width: '100%' }}
              />
            </div>
          </div>

          <h3 style={{ margin: '20px 0 12px 0', fontSize: 15 }}>2️⃣ Пароль подключения</h3>
          <div style={{ marginBottom: 8, maxWidth: 420 }}>
            <input
              type="password"
              className="input"
              autoComplete="new-password"
              placeholder={status?.has_password ? 'Оставьте пустым, чтобы сохранить текущий пароль' : 'Пароль от сервера csqtt'}
              value={password}
              onChange={(e) => {
                wizardTouchedRef.current = true
                setPassword(e.target.value)
              }}
              style={{ width: '100%' }}
            />
          </div>
          <p className="muted" style={{ fontSize: 13, marginTop: 0, marginBottom: 0 }}>
            {status?.has_password
              ? '🔑 Пароль уже сохранён на роутере — можно оставить поле пустым или ввести новый.'
              : '🔑 Это пароль туннеля, заданный при установке сервера csqtt (или выданный владельцем сервера). Адрес внутри туннеля сервер выдаст сам.'}
          </p>

          <h3 style={{ margin: '20px 0 12px 0', fontSize: 15 }}>3️⃣ Авторизация ВКонтакте и звонки</h3>
          {renderVkAccountCard()}

          {(status?.has_token || profileToken || status?.has_profile_token) && (
            <div className="vk-source-pills" role="group" aria-label="Источник звонков" style={{ marginTop: 12 }}>
              {status?.has_token && (
                <label className={`vk-source-pill${selectedSource === 'oauth' ? ' active' : ''}`}>
                  <input
                    type="radio"
                    name="vkturn-source"
                    checked={selectedSource === 'oauth'}
                    onChange={() => setSourceMode('oauth')}
                    style={{ marginRight: 6 }}
                  />
                  Мой аккаунт VK
                </label>
              )}
              {(profileToken || (profileToken === null && status?.has_profile_token)) && (
                <label className={`vk-source-pill${selectedSource === 'profile_token' ? ' active' : ''}`}>
                  <input
                    type="radio"
                    name="vkturn-source"
                    checked={selectedSource === 'profile_token'}
                    onChange={() => setSourceMode('profile_token')}
                    style={{ marginRight: 6 }}
                  />
                  Токен профиля
                </label>
              )}
              <label className={`vk-source-pill${selectedSource === 'manual' ? ' active' : ''}`}>
                <input
                  type="radio"
                  name="vkturn-source"
                  checked={selectedSource === 'manual'}
                  onChange={() => setSourceMode('manual')}
                  style={{ marginRight: 6 }}
                />
                Ссылки на звонки
              </label>
            </div>
          )}

          {selectedSource === 'manual' && (
            <>
              <p className="muted" style={{ fontSize: 13, marginTop: 12, marginBottom: 12 }}>
                💡 Или укажите прямые ссылки на звонки ВК вручную (до {maxLinks} ссылок):
              </p>

              <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginBottom: 14 }}>
                {callLinks.map((link, idx) => (
                  <div key={idx} style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                    <input
                      type="text"
                      className="input"
                      placeholder={idx === 0 ? 'Основная ссылка на звонок (https://vk.com/call/join/...)' : 'Запасная ссылка'}
                      value={link}
                      onChange={(e) => handleCallLinkChange(idx, e.target.value)}
                      style={{ flex: 1 }}
                    />
                    {callLinks.length > 1 && (
                      <button
                        type="button"
                        className="btn btn-secondary"
                        onClick={() => setCallLinks((prev) => prev.filter((_, i) => i !== idx))}
                        title="Удалить ссылку"
                        style={{ padding: '6px 10px' }}
                      >
                        ✕
                      </button>
                    )}
                  </div>
                ))}
                {callLinks.length < maxLinks && (
                  <div>
                    <button
                      type="button"
                      className="btn btn-secondary"
                      onClick={() => setCallLinks((prev) => [...prev, ''])}
                      style={{ fontSize: 13 }}
                    >
                      + Добавить запасную ссылку
                    </button>
                  </div>
                )}
              </div>
            </>
          )}

          <div style={{ marginTop: 20, paddingTop: 16, borderTop: '1px solid var(--border)' }}>
            <button
              type="button"
              className="btn btn-primary"
              disabled={actionPending}
              onClick={handleWizardEnable}
              style={{ padding: '10px 24px', fontSize: 14, fontWeight: 600 }}
            >
              {actionPending ? (installing ? '⏳ Скачиваю компонент…' : '⏳ Активация…') : 'Включить'}
            </button>
            {installing && (
              <div className="muted" style={{ marginTop: 10, fontSize: 13 }} data-testid="install-progress">
                ⏳ Скачиваю компонент VK-туннеля: {installProgressText(status?.install)}
              </div>
            )}
          </div>
        </section>
        {renderAuthModal()}
      </div>
    )
  }

  // ---------------------------------------------------------
  // B. Карточка мониторинга
  // ---------------------------------------------------------
  const phase = status?.phase || 'disabled'
  const isRunning = phase === 'connected' && Boolean(status?.tunnel_ready) && (status?.connected_streams ?? 0) > 0
  const waitingTunnel = Boolean(status?.enabled && !status?.tunnel_ready && phase !== 'not_installed')
  const isVkAuthFailure = status?.error_source === 'vk' && phase === 'auth_failed'
  const showPasswordFix = !isVkAuthFailure && (phase === 'auth_failed' || phase === 'server_unreachable')
  const showLastError = Boolean(
    status?.enabled && status?.last_error && !['captcha_required', 'disabled', 'not_installed'].includes(phase),
  )
  const showManualLinksSection = effectiveAuthMode === 'manual' || manualSwitchPending

  const targetStreams = status?.target_streams ?? DEFAULT_STREAMS
  const displayedStreamsStr = draftStreamsInput ?? String(targetStreams)
  const parsedDraftStreams =
    draftStreamsInput !== null && /^\d+$/.test(draftStreamsInput.trim()) ? Number(draftStreamsInput.trim()) : null
  const rangeStreams =
    parsedDraftStreams !== null ? Math.max(3, Math.min(100, parsedDraftStreams)) : targetStreams
  const canSaveStreams =
    !streamsUpdating &&
    Boolean(status?.enabled) &&
    parsedDraftStreams !== null &&
    parsedDraftStreams >= 3 &&
    parsedDraftStreams <= 100 &&
    parsedDraftStreams !== targetStreams

  return (
    <div className="vkturn-container" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <section
        className={`vk-section ${isRunning ? 'vk-alert-ok' : ''}`}
        style={{ padding: '20px 24px' }}
      >
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 14 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap', marginBottom: 6 }}>
              <h2 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>VK-туннель</h2>
              <span className="muted" style={{ fontSize: 12 }}>
                Ядро csqtt {status?.core_version ? `· ${status.core_version}` : ''} {status?.pid ? `(PID: ${status.pid})` : ''}
              </span>
              {renderPhaseBadge(phase, status?.connected_streams)}
            </div>
            <div className="muted" style={{ fontSize: 13 }}>
              Звонки ВКонтакте → сервер csqtt: <b>{status?.peer || 'не указан'}</b>
              {isRunning && (
                <span data-testid="traffic-line">
                  {' '}• ↑ {formatTraffic(status?.bytes_up)} ↓ {formatTraffic(status?.bytes_down)}
                </span>
              )}
            </div>
          </div>

          <label className="switch" title={status?.enabled ? 'Отключить туннель' : 'Включить туннель'}>
            <input
              type="checkbox"
              aria-label="Включить или отключить VK-туннель"
              checked={Boolean(status?.enabled)}
              disabled={actionPending}
              onChange={(e) => handleToggleEnabled(e.target.checked)}
            />
            <span className="slider" />
          </label>
        </div>

        {/* Единый переключатель источника звонков */}
        <div
          data-testid="vk-source-switcher"
          style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 10, marginTop: 14, paddingTop: 14, borderTop: '1px solid var(--border)' }}
        >
          <span className="muted" style={{ fontSize: 13, fontWeight: 600 }}>Источник звонков:</span>
          <div className="vk-source-pills">
            <button
              type="button"
              className={`vk-source-pill${effectiveAuthMode === 'oauth' && !manualSwitchPending ? ' active' : ''}`}
              disabled={actionPending}
              onClick={() => void handleSwitchAuthMode('oauth')}
            >
              🔵 Аккаунт VK
            </button>
            {status?.has_profile_token && (
              <button
                type="button"
                className={`vk-source-pill${effectiveAuthMode === 'profile_token' && !manualSwitchPending ? ' active' : ''}`}
                disabled={actionPending}
                onClick={() => void handleSwitchAuthMode('profile_token')}
              >
                🔑 Токен профиля
              </button>
            )}
            <button
              type="button"
              className={`vk-source-pill${showManualLinksSection ? ' active' : ''}`}
              disabled={actionPending}
              onClick={() => void handleSwitchAuthMode('manual')}
            >
              🔗 Ручные ссылки
            </button>
          </div>
        </div>

        <div style={{ marginTop: 14 }}>
          {renderVkAccountCard()}
        </div>

        {installing && (
          <div className="vk-alert vk-alert-warn" style={{ marginTop: 14, fontSize: 13.5 }} data-testid="install-progress">
            ⏳ Скачиваю компонент VK-туннеля: {installProgressText(status?.install)}
          </div>
        )}

        {phase === 'not_installed' && !installing && (
          <div
            className="vk-alert vk-alert-warn"
            style={{ marginTop: 14, fontSize: 13.5, display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}
          >
            <div>Компонент ядра VK-туннеля не установлен на роутере.</div>
            <button
              type="button"
              className="btn btn-primary"
              disabled={actionPending}
              onClick={handleInstallCore}
              style={{ fontSize: 13 }}
            >
              Установить компонент
            </button>
          </div>
        )}

        {waitingTunnel && phase === 'connected' && (
          <div className="vk-alert vk-alert-warn" style={{ marginTop: 14, fontSize: 13.5 }} data-testid="waiting-tunnel">
            ⏳ Соединение с сервером есть, подключаю туннель к роутеру — узел заработает в списке серверов автоматически.
          </div>
        )}

        {phase === 'captcha_required' && (
          <div className="vk-alert vk-alert-danger" style={{ marginTop: 14, fontSize: 13.5, color: 'var(--danger-fg, #ef4444)' }}>
            ⚠️ ВКонтакте запросил капчу, и автоматически пройти её не удалось. Повторная попытка будет предпринята
            автоматически через 10 минут.
          </div>
        )}

        {isVkAuthFailure && (
          <div className="vk-alert vk-alert-danger" style={{ marginTop: 14, fontSize: 13.5 }} data-testid="vk-auth-failure">
            <div style={{ marginBottom: 8, fontWeight: 600, color: 'var(--danger-fg, #ef4444)' }}>
              ⚠️ {status?.last_error || 'Вход в аккаунт VK отклонён: аккаунт заблокирован или токен отозван.'}
            </div>
            {effectiveAuthMode === 'profile_token' ? (
              <div style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={handleStartVkOAuth}
                  style={{ fontSize: 13 }}
                >
                  Войти через VK ID
                </button>
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => void handleSwitchAuthMode('manual')}
                  style={{ fontSize: 13 }}
                >
                  Перейти на ручные ссылки
                </button>
              </div>
            ) : effectiveAuthMode === 'manual' ? (
              <div style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={handleStartVkOAuth}
                  style={{ fontSize: 13 }}
                >
                  Войти через VK ID
                </button>
              </div>
            ) : (
              <div style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={handleStartVkOAuth}
                  style={{ fontSize: 13 }}
                >
                  Войти под другим аккаунтом VK
                </button>
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={actionPending}
                  onClick={handleLogoutVk}
                  style={{ fontSize: 13 }}
                >
                  Сбросить авторизацию
                </button>
              </div>
            )}
          </div>
        )}

        {showPasswordFix && (
          <div className="vk-alert vk-alert-danger" style={{ marginTop: 14, fontSize: 13.5 }} data-testid="password-fix">
            <div style={{ marginBottom: 10, fontWeight: 600, color: 'var(--danger-fg, #ef4444)' }}>
              {phase === 'auth_failed'
                ? status?.last_error || 'Сервер csqtt отклонил пароль подключения.'
                : 'Сервер на VPS не отвечает. Проверьте, что сервер csqtt запущен и открыт его UDP-порт. ' +
                  'При неверном пароле сервер тоже молчит — если пароль или адрес могли измениться, обновите их ниже.'}
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap', marginBottom: 8 }}>
              <input
                type="text"
                className="input"
                data-testid="quick-fix-peer-input"
                placeholder="Адрес:порт или csqtt://..."
                value={editPeerInput}
                onChange={(e) => {
                  peerEditedRef.current = true
                  setEditPeerInput(e.target.value)
                }}
                style={{ flex: '1 1 220px' }}
              />
              <button
                type="button"
                className="btn btn-secondary"
                disabled={updatingPeer || !editPeerInput.trim()}
                onClick={handleUpdatePeer}
                style={{ fontSize: 13 }}
              >
                {updatingPeer ? 'Сохранение…' : 'Сохранить адрес'}
              </button>
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <input
                type="password"
                className="input"
                autoComplete="new-password"
                placeholder="Новый пароль подключения"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                style={{ flex: 1 }}
              />
              <button
                type="button"
                className="btn btn-primary"
                disabled={updatingPassword || !newPassword.trim()}
                onClick={handleUpdatePassword}
                style={{ fontSize: 13 }}
              >
                {updatingPassword ? 'Сохранение…' : 'Сохранить пароль'}
              </button>
            </div>
          </div>
        )}

        {phase === 'link_invalid' && (
          <div className="vk-alert vk-alert-danger" style={{ marginTop: 14, fontSize: 13.5 }}>
            <div style={{ marginBottom: 8, fontWeight: 600, color: 'var(--danger-fg, #ef4444)' }}>
              Ссылка на звонок больше не действует или звонок был завершён.
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <input
                type="text"
                className="input"
                placeholder="Новая ссылка на звонок (https://vk.com/call/join/...)"
                value={newLinkInput}
                onChange={(e) => setNewLinkInput(e.target.value)}
                style={{ flex: 1 }}
              />
              <button
                type="button"
                className="btn btn-primary"
                disabled={updatingLink || !newLinkInput.trim()}
                onClick={() => void handleUpdateCallLink(false)}
                style={{ fontSize: 13 }}
              >
                {updatingLink ? 'Обновление…' : 'Обновить ссылку'}
              </button>
            </div>
          </div>
        )}

        {showLastError && !showPasswordFix && (
          <div className="muted" data-testid="last-error" style={{ marginTop: 12, fontSize: 13 }}>
            ⚠️ {status?.last_error}
          </div>
        )}

        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginTop: 18, flexWrap: 'wrap' }}>
          <button
            type="button"
            className="btn btn-secondary"
            disabled={testingPing || actionPending || !status?.enabled || !status?.tunnel_ready}
            onClick={handleTest}
            title={status?.tunnel_ready ? '' : 'Станет доступно, когда туннель подключится к серверу'}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
          >
            <span>{testingPing ? '⏳' : '⚡'}</span>
            <span>{testingPing ? 'Проверяю…' : 'Проверить'}</span>
          </button>

          <button
            type="button"
            className="btn btn-secondary"
            disabled={actionPending || !status?.enabled}
            onClick={handleRestart}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
          >
            <span>🔄</span>
            <span>Перезапустить</span>
          </button>

          <button
            type="button"
            className="btn btn-secondary"
            disabled={actionPending}
            onClick={handleResetServer}
            title="Отвязать текущий сервер csqtt и вернуться к мастеру настройки"
            data-testid="reset-server-btn"
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 6,
              color: 'var(--danger-fg, #ef4444)',
              borderColor: 'rgba(239, 68, 68, 0.3)',
            }}
          >
            <span>🗑️</span>
            <span>Сбросить сервер</span>
          </button>

          {testResult !== null && (
            <span className={testResult.delay === null ? 'badge badge-danger' : 'badge badge-success'} data-testid="latency-badge" style={{ fontSize: 13 }}>
              {testResult.delay === null ? 'Последняя проверка: ошибка' : `Задержка ${testResult.delay} мс ✓`} · {testResult.at.toLocaleTimeString()}
            </span>
          )}
        </div>
      </section>

      <section className="vk-section" style={{ padding: '18px 24px' }}>
        <div
          role="button"
          tabIndex={0}
          onClick={() => {
            const next = !accordionOpen
            setAccordionOpen(next)
            if (next && logs.length === 0) {
              fetchLogs()
            }
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault()
              setAccordionOpen((prev) => !prev)
            }
          }}
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            cursor: 'pointer',
            userSelect: 'none',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 18 }}>📋</span>
            <span style={{ fontWeight: 600, fontSize: 15 }}>Настройки туннеля и подробности</span>
          </div>
          <span style={{ fontSize: 16, transform: accordionOpen ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s' }}>
            ▼
          </span>
        </div>

        {accordionOpen && (
          <div style={{ marginTop: 18, paddingTop: 16, borderTop: '1px solid var(--border)' }}>
            <div style={{ marginBottom: 20, maxWidth: 640 }} data-testid="stream-controls">
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 8, marginBottom: 8 }}>
                <label htmlFor="vkturn-stream-range" style={{ fontSize: 13.5, fontWeight: 600 }}>
                  Мощность туннеля (потоки):{' '}
                  <span style={{ color: 'var(--accent, #3b82f6)', fontWeight: 700, fontSize: 16 }}>
                    {displayedStreamsStr || targetStreams}
                  </span>
                </label>
                <span className="badge badge-neutral" style={{ fontSize: 12 }}>
                  Активно: {status?.connected_streams ?? 0} из {targetStreams}
                </span>
              </div>
              <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
                <input
                  id="vkturn-stream-range"
                  type="range"
                  min={3}
                  max={100}
                  step={1}
                  className="input"
                  data-testid="stream-range"
                  value={rangeStreams}
                  disabled={streamsUpdating || !status?.enabled}
                  onChange={(e) => setDraftStreamsInput(e.target.value)}
                  style={{ flex: '1 1 200px', maxWidth: 300, cursor: 'pointer' }}
                />
                <input
                  id="vkturn-stream-count"
                  type="number"
                  min={3}
                  max={100}
                  className="input"
                  data-testid="stream-count"
                  value={displayedStreamsStr}
                  disabled={streamsUpdating || !status?.enabled}
                  onChange={(e) => setDraftStreamsInput(e.target.value)}
                  style={{ width: 85, fontWeight: 600 }}
                />
                <button
                  type="button"
                  className="btn btn-primary"
                  data-testid="save-streams-btn"
                  disabled={!canSaveStreams}
                  onClick={() => handleSaveStreams()}
                  style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
                >
                  <span>💾 Сохранить</span>
                </button>
              </div>
              <div className="muted" style={{ fontSize: 12, marginTop: 8, lineHeight: 1.4 }}>
                Установите нужное число потоков и нажмите «Сохранить». Потоки применяются однократно без промежуточных перезапусков.
              </div>
            </div>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
                gap: 14,
                marginBottom: 20,
              }}
            >
              {showManualLinksSection && (
                <div className="vk-subcard">
                  <div className="muted" style={{ fontSize: 12 }}>Ссылки на звонки</div>
                  <div style={{ fontSize: 18, fontWeight: 700, marginTop: 4 }}>
                    {status?.links_count ?? 0} из {maxLinks}
                  </div>
                </div>
              )}
              <div className="vk-subcard">
                <div className="muted" style={{ fontSize: 12 }}>Число перезапусков</div>
                <div style={{ fontSize: 18, fontWeight: 700, marginTop: 4 }}>{status?.restarts ?? 0}</div>
              </div>
              <div className="vk-subcard">
                <div className="muted" style={{ fontSize: 12 }}>Адрес роутера в туннеле</div>
                <div style={{ fontSize: 15, fontWeight: 600, marginTop: 4, fontFamily: 'monospace' }} data-testid="tunnel-ip">
                  {status?.tunnel_ip ? `${status.tunnel_ip} · ${status.interface}` : 'выдаст сервер при подключении'}
                </div>
              </div>
            </div>

            {showManualLinksSection && (
              <div style={{ marginBottom: 20 }}>
                <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Ссылки на звонки</div>
                {(status?.links || []).length > 0 ? (
                  <ul style={{ margin: '0 0 12px 0', paddingLeft: 18, fontSize: 13, fontFamily: 'monospace' }} data-testid="links-list">
                    {(status?.links || []).map((l, i) => (
                      <li key={`${l}-${i}`} style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                        <span>{l}{i === 0 ? ' (основная)' : ''}</span>
                        {((status?.links?.length ?? 0) > 1 || !status?.enabled) && (
                          <button
                            type="button"
                            className="btn btn-secondary"
                            disabled={updatingLink || actionPending}
                            onClick={() => void handleRemoveLink(i)}
                            title="Удалить ссылку"
                            style={{ padding: '1px 7px', fontSize: 11 }}
                          >
                            ✕
                          </button>
                        )}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <div className="muted" data-testid="links-list" style={{ fontSize: 12.5, marginBottom: 10 }}>
                    Ссылки ещё не добавлены. Укажите рабочую ссылку на звонок ВКонтакте.
                  </div>
                )}

                {phase !== 'link_invalid' && (
                  <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap', maxWidth: 680 }}>
                    <input
                      type="text"
                      className="input"
                      placeholder="Добавить ссылку (https://vk.com/call/join/...)"
                      value={newLinkInput}
                      onChange={(e) => setNewLinkInput(e.target.value)}
                      style={{ flex: '1 1 260px' }}
                    />
                    <button
                      type="button"
                      className="btn btn-secondary"
                      disabled={updatingLink || !newLinkInput.trim()}
                      onClick={() => void handleUpdateCallLink(false)}
                      style={{ fontSize: 13 }}
                    >
                      Основная
                    </button>
                    {(status?.links?.length ?? 0) < maxLinks && (
                      <button
                        type="button"
                        className="btn btn-secondary"
                        disabled={updatingLink || !newLinkInput.trim()}
                        onClick={() => void handleUpdateCallLink(true)}
                        style={{ fontSize: 13 }}
                      >
                        + Запасная
                      </button>
                    )}
                  </div>
                )}
              </div>
            )}

            {!showPasswordFix && (
              <div style={{ marginBottom: 20 }}>
                <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Адрес сервера csqtt</div>
                <div style={{ display: 'flex', gap: 8, alignItems: 'center', maxWidth: 640, marginBottom: 16 }}>
                  <input
                    type="text"
                    className="input"
                    data-testid="accordion-peer-input"
                    placeholder="host:port или ссылка csqtt://..."
                    value={editPeerInput}
                    onChange={(e) => {
                      peerEditedRef.current = true
                      setEditPeerInput(e.target.value)
                    }}
                    style={{ flex: 1 }}
                  />
                  <button
                    type="button"
                    className="btn btn-secondary"
                    disabled={updatingPeer || !editPeerInput.trim()}
                    onClick={handleUpdatePeer}
                    style={{ fontSize: 13 }}
                  >
                    {updatingPeer ? 'Сохранение…' : 'Сохранить адрес'}
                  </button>
                </div>

                <div data-testid="password-change">
                  <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Пароль подключения к серверу</div>
                  <div style={{ display: 'flex', gap: 8, alignItems: 'center', maxWidth: 640 }}>
                    <input
                      type="password"
                      className="input"
                      autoComplete="new-password"
                      placeholder="Сменить пароль подключения"
                      value={newPassword}
                      onChange={(e) => setNewPassword(e.target.value)}
                      style={{ flex: 1 }}
                    />
                    <button
                      type="button"
                      className="btn btn-secondary"
                      disabled={updatingPassword || !newPassword.trim()}
                      onClick={handleUpdatePassword}
                      style={{ fontSize: 13 }}
                    >
                      {updatingPassword ? 'Сохранение…' : 'Сменить пароль'}
                    </button>
                  </div>
                </div>
              </div>
            )}

            <div>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
                <span style={{ fontSize: 13, fontWeight: 600 }}>Логи службы (Tail)</span>
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={loadingLogs}
                  onClick={fetchLogs}
                  style={{ fontSize: 12, padding: '4px 8px' }}
                >
                  {loadingLogs ? 'Обновление…' : '🔄 Обновить логи'}
                </button>
              </div>
              <pre
                data-testid="log-tail-pre"
                style={{
                  background: 'var(--code-bg, #090d16)',
                  color: 'var(--code-fg, #e2e8f0)',
                  padding: 12,
                  borderRadius: 8,
                  fontSize: 12,
                  fontFamily: 'Consolas, monospace',
                  maxHeight: 260,
                  overflowY: 'auto',
                  border: '1px solid var(--border)',
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-all',
                }}
              >
                {logs.length > 0 ? logs.join('\n') : 'Логи пока пусты.'}
              </pre>
            </div>
          </div>
        )}
      </section>
      {renderAuthModal()}
    </div>
  )
}
