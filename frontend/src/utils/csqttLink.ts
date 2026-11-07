export interface ParsedCsqttProfile {
  host: string;
  port: number;
  password: string;
  hashes?: string[];
}

const KNOWN_KEYS = ['host', 'peer', 'password', 'hashes'];

function separateParams(link: string): string {
  let result = link;
  for (const key of KNOWN_KEYS) {
    // В Telegram ссылки иногда склеиваются без '&'
    result = result.replace(new RegExp(`([?&]|v=2)${key}=`, 'g'), `$1&${key}=`);
    result = result.replace(new RegExp(`([^?&])${key}=`, 'g'), `$1&${key}=`);
  }
  return result;
}

export function parseCsqttLink(raw: string): ParsedCsqttProfile | null {
  const trimmed = raw.trim();
  if (!trimmed.toLowerCase().startsWith('csqtt://')) {
    return null;
  }

  // Формат v2: csqtt://connect?v=2&host=...&peer=46000&password=...
  if (trimmed.toLowerCase().startsWith('csqtt://connect?')) {
    const fixed = separateParams(trimmed);
    const queryPart = fixed.substring(fixed.indexOf('?') + 1);
    const params = new URLSearchParams(queryPart);

    const host = params.get('host')?.trim();
    const peerStr = params.get('peer')?.trim();
    const password = params.get('password')?.trim();
    const hashesStr = params.get('hashes')?.trim();

    if (!host || !password) {
      return null;
    }

    const port = peerStr ? parseInt(peerStr, 10) : 46000;
    if (isNaN(port) || port <= 0 || port > 65535) {
      return null;
    }

    const hashes = hashesStr
      ? hashesStr.split(',').map((h) => h.trim()).filter((h) => h.length > 0)
      : undefined;

    return {
      host,
      port,
      password,
      hashes,
    };
  }

  // Формат legacy: csqtt://password@host:port
  const withoutScheme = trimmed.substring('csqtt://'.length);
  const atIndex = withoutScheme.lastIndexOf('@');
  if (atIndex <= 0 || atIndex === withoutScheme.length - 1) {
    return null;
  }

  let password = '';
  try {
    password = decodeURIComponent(withoutScheme.substring(0, atIndex));
  } catch {
    password = withoutScheme.substring(0, atIndex);
  }

  const address = withoutScheme.substring(atIndex + 1);
  let host = '';
  let port: number = 46000;

  if (address.startsWith('[')) {
    const closeBracket = address.indexOf(']');
    if (closeBracket === -1) return null;
    host = address.substring(1, closeBracket);
    const rest = address.substring(closeBracket + 1);
    if (rest.startsWith(':')) {
      const p = parseInt(rest.substring(1), 10);
      if (!isNaN(p) && p > 0 && p <= 65535) port = p;
    }
  } else {
    const parts = address.split(':');
    host = parts[0];
    if (parts.length > 1) {
      const p = parseInt(parts[1], 10);
      if (!isNaN(p) && p > 0 && p <= 65535) port = p;
    }
  }

  if (!host || !password) {
    return null;
  }

  return {
    host,
    port,
    password,
  };
}
