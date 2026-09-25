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
})
