export interface ParsedCsqttProfile {
  host: string;
  port: number;
  password: string;
  hashes?: string[];
  token?: string;
  power?: number;
}

function repairGluedQuery(query: string): string | null {
  const match = /^v=2host=([a-z0-9.-]+)&?peer=([0-9]+)&?password=([^&]+)$/i.exec(query);
  if (!match || /(?:v|host|peer|password|client|hashes|token|workers|power)=/i.test(match[3])) return null;
  return `v=2&host=${match[1]}&peer=${match[2]}&password=${match[3]}`;
}

function parsePort(value: string): number | null {
  if (!/^\d+$/.test(value)) return null;
  const port = Number(value);
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

function isUsableHost(host: string): boolean {
  if (['localhost', '127.0.0.1', '0.0.0.0', '::1'].includes(host.toLowerCase())) return false;
  return /^[a-z0-9.-]{1,253}$/i.test(host) || /^[a-f0-9:]+$/i.test(host) && host.includes(':');
}

export function parseCsqttLink(raw: string): ParsedCsqttProfile | null {
  const trimmed = raw.trim();
  if (!trimmed.toLowerCase().startsWith('csqtt://')) {
    return null;
  }

  // Формат v2: csqtt://connect?v=2&host=...&peer=46000&password=...
  if (trimmed.toLowerCase().startsWith('csqtt://connect?')) {
    let queryPart = trimmed.substring(trimmed.indexOf('?') + 1);
    // A literal plus is data in CSQTT links, not a form-encoded space.
    let params = new URLSearchParams(queryPart.replace(/\+/g, '%2B'));
    // Only attempt the legacy repair when the structured query is incomplete.
    if (!params.has('host') || !params.has('peer') || !params.has('password')) {
      const fixed = repairGluedQuery(queryPart);
      if (!fixed) return null;
      queryPart = fixed;
      params = new URLSearchParams(queryPart.replace(/\+/g, '%2B'));
    }

    if (/%(?![0-9a-fA-F]{2})/.test(queryPart) || params.get('v') !== '2') return null;
    const seen = new Set<string>();
    for (const key of params.keys()) {
      if (seen.has(key)) return null;
      seen.add(key);
    }
    if (params.has('power') && params.has('workers')) return null;

    const host = params.get('host');
    const peerStr = params.get('peer')?.trim();
    const password = params.get('password')?.trim();
    const hashesRaw = queryPart.split('&').find((part) => part.startsWith('hashes='))?.slice(7);

    if (!host || !isUsableHost(host) || !password) {
      return null;
    }

    const port = peerStr ? parsePort(peerStr) : null;
    if (port === null) {
      return null;
    }

    let hashes: string[] | undefined;
    if (hashesRaw !== undefined) {
      try {
        hashes = hashesRaw.split(/[+,]/).map((h) => decodeURIComponent(h).trim()
          .replace(/^(?:https?:\/\/)?(?:m\.)?vk\.(?:com|ru)\/call\/join\//i, '')
          .replace(/[?#].*$/, '').replace(/\/$/, ''));
      } catch {
        return null;
      }
      if (hashes.length > 6 || hashes.some((h) => !/^[a-z0-9_-]{16,128}$/i.test(h)) || new Set(hashes).size !== hashes.length) return null;
    }
    const token = params.get('token') ?? undefined;
    if (token !== undefined && (!token || token.length > 4096 || /\s/.test(token))) return null;
    if (token && hashes?.length) return null;
    const rawPower = params.get('power') ?? params.get('workers');
    let power: number | undefined;
    if (rawPower !== null) {
      if (!/^\d+$/.test(rawPower)) return null;
      const value = Number(rawPower);
      if (!Number.isSafeInteger(value) || value <= 0) return null;
      power = Math.min(100, Math.max(10, value));
    }

    return {
      host,
      port,
      password,
      hashes,
      token,
      power,
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
  let port: number | null = null;

  if (address.startsWith('[')) {
    const closeBracket = address.indexOf(']');
    if (closeBracket === -1) return null;
    host = address.substring(1, closeBracket);
    const rest = address.substring(closeBracket + 1);
    if (rest.startsWith(':')) port = parsePort(rest.substring(1));
  } else {
    const parts = address.split(':');
    host = parts[0];
    if (parts.length === 2) port = parsePort(parts[1]);
  }

  if (!host || !isUsableHost(host) || !password || port === null) {
    return null;
  }

  return {
    host,
    port,
    password,
  };
}
