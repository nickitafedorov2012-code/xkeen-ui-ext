export interface ParsedBackupPayload {
  name: string
  files: Record<string, string>
  [key: string]: unknown
}

export function validateAndParseBackupPayload(
  text: string,
  fileName: string
): ParsedBackupPayload {
  let payload: unknown
  try {
    payload = JSON.parse(text)
  } catch {
    throw new Error('Файл не является корректным JSON/xkbak архивом')
  }
  if (
    !payload ||
    typeof payload !== 'object' ||
    !('files' in payload) ||
    !(payload as { files?: unknown }).files ||
    typeof (payload as { files?: unknown }).files !== 'object'
  ) {
    throw new Error('Некорректный формат архива бэкапа: отсутствует секция files')
  }
  const record = payload as Record<string, unknown>
  const name =
    typeof record.name === 'string' && record.name.length > 0
      ? record.name
      : fileName.replace(/\.[^/.]+$/, '')
  return {
    ...record,
    name,
    files: record.files as Record<string, string>,
  }
}

export function sanitizeBackupName(rawName: string): string {
  const filtered = rawName.replace(/[^a-zA-Z0-9_-]/g, '')
  const base = filtered.replace(/^xr-/, '').replace(/^[-_]+|[-_]+$/g, '')
  if (!base) {
    return 'xr-imported-fallback'
  }
  return filtered.startsWith('xr-') ? filtered : `xr-${filtered}`
}
