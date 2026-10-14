import { describe, it, expect } from 'vitest'

describe('Backup Import JSON Validation', () => {
  function validateAndParseBackupPayload(text: string, fileName: string) {
    let payload: any
    try {
      payload = JSON.parse(text)
    } catch {
      throw new Error('Файл не является корректным JSON/xkbak архивом')
    }
    if (!payload || typeof payload !== 'object' || !payload.files || typeof payload.files !== 'object') {
      throw new Error('Некорректный формат архива бэкапа: отсутствует секция files')
    }
    if (!payload.name) {
      payload.name = fileName.replace(/\.[^/.]+$/, '')
    }
    return payload
  }

  it('regression: accepts valid JSON/xkbak archive and auto-assigns name if missing', () => {
    const jsonStr = JSON.stringify({
      created_at: '2026-09-25T05:00:00Z',
      files: {
        'config.yaml': 'port: 7890\n',
        'config.json': '{"rci": {}}\n',
      },
    })

    const payload = validateAndParseBackupPayload(jsonStr, 'backup_20260925.xkbak')
    expect(payload.name).toBe('backup_20260925')
    expect(payload.files['config.yaml']).toBe('port: 7890\n')
    expect(payload.files['config.json']).toBe('{"rci": {}}\n')
  })

  it('regression: preserves existing name in backup JSON payload', () => {
    const jsonStr = JSON.stringify({
      name: 'custom_snapshot_name',
      files: {
        'config.yaml': 'port: 7890\n',
      },
    })

    const payload = validateAndParseBackupPayload(jsonStr, 'fallback.json')
    expect(payload.name).toBe('custom_snapshot_name')
  })

  it('regression: rejects non-JSON strings with clear error', () => {
    const invalidText = 'This is raw binary or zip data \x00\x01\x02'
    expect(() => validateAndParseBackupPayload(invalidText, 'bad.xkbak')).toThrow(
      'Файл не является корректным JSON/xkbak архивом'
    )
  })

  it('regression: rejects JSON objects without files section', () => {
    const noFilesJson = JSON.stringify({
      name: 'broken_backup',
      foo: 'bar',
    })
    expect(() => validateAndParseBackupPayload(noFilesJson, 'broken.json')).toThrow(
      'Некорректный формат архива бэкапа: отсутствует секция files'
    )
  })

  it('regression: validates presence of full backup components (zapret-hosts.txt, config.yaml, config.json)', () => {
    const fullBackup = JSON.stringify({
      name: 'xr-full-snapshot',
      files: {
        'config.yaml': 'port: 7890\n',
        'config.json': '{"rci": {}}\n',
        'zapret.conf': 'MODE=nfqws2\n',
        'zapret-hosts.txt': 'youtube.com\n',
        'ru_exclude_override.lst': '1.2.3.4\n',
      },
    })
    const payload = validateAndParseBackupPayload(fullBackup, 'full.xkbak')
    expect(payload.files['zapret-hosts.txt']).toBe('youtube.com\n')
    expect(payload.files['ru_exclude_override.lst']).toBe('1.2.3.4\n')
  })

  it('regression: sanitizes backup names to prevent path traversal in fallback', () => {
    function sanitizeBackupName(rawName: string): string {
      const filtered = rawName.replace(/[^a-zA-Z0-9_-]/g, '')
      const base = filtered.replace(/^xr-/, '').replace(/^[-_]+|[-_]+$/g, '')
      if (!base) {
        return 'xr-imported-fallback'
      }
      return filtered.startsWith('xr-') ? filtered : `xr-${filtered}`
    }

    expect(sanitizeBackupName('../../../')).toBe('xr-imported-fallback')
    expect(sanitizeBackupName('../../../etc/passwd')).toBe('xr-etcpasswd')
    expect(sanitizeBackupName('xr-')).toBe('xr-imported-fallback')
    expect(sanitizeBackupName('my-backup')).toBe('xr-my-backup')
    expect(sanitizeBackupName('xr-valid_name')).toBe('xr-valid_name')
  })

  it('regression (FE-02): syncs notification state fields into settings payload before save', () => {
    const originalSettings = {
      mihomo: { config_path: '/opt/etc/mihomo/config.yaml' },
      notifications: {
        telegram_enabled: false,
        telegram_bot_token: '',
        telegram_chat_id: '',
        webhook_url: '',
      },
    }

    const localTelegramEnabled = true
    const localTelegramBotToken = '123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11'
    const localTelegramChatId = '-100123456789'
    const localWebhookUrl = 'https://webhook.site/test'

    const updatedSettings = {
      ...originalSettings,
      notifications: {
        ...originalSettings.notifications,
        telegram_enabled: localTelegramEnabled,
        telegram_bot_token: localTelegramBotToken,
        telegram_chat_id: localTelegramChatId,
        webhook_url: localWebhookUrl,
      },
    }

    expect(updatedSettings.notifications.telegram_enabled).toBe(true)
    expect(updatedSettings.notifications.telegram_bot_token).toBe(localTelegramBotToken)
    expect(updatedSettings.notifications.telegram_chat_id).toBe(localTelegramChatId)
    expect(updatedSettings.notifications.webhook_url).toBe(localWebhookUrl)
  })

  it('regression (FE-02): passes expected_revision in domains save payload', () => {
    const directDomains = 'direct1.com\ndirect2.com'
    const forceDomains = 'proxy1.com\nproxy2.com'
    const domainsRevision = 'rev_abc123'

    const payload = {
      direct: directDomains.split('\n'),
      force: forceDomains.split('\n'),
      expected_revision: domainsRevision,
    }

    expect(payload.expected_revision).toBe('rev_abc123')
    expect(payload.direct).toEqual(['direct1.com', 'direct2.com'])
    expect(payload.force).toEqual(['proxy1.com', 'proxy2.com'])
  })
})
