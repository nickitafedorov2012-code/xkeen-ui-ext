import { useState, useMemo } from 'react'
import { parseMultipleLinks, type ParsedNode } from '../utils/nodeParser'
import { apiPost, decryptCsqttProfile, saveVkTurnConfig } from '../api'
import { copyToClipboard } from '../utils/clipboard'
import { parseCsqttLink } from '../utils/csqttLink'

interface OutboundGeneratorModalProps {
  isOpen?: boolean
  onClose: () => void
  onImportSuccess?: () => void
  notify: (msg: string, error?: boolean) => void
}

export default function OutboundGeneratorModal({
  isOpen = true,
  onClose,
  onImportSuccess,
  notify,
}: OutboundGeneratorModalProps) {
  const [inputText, setInputText] = useState('')
  const [target, setTarget] = useState<'config' | 'provider'>('config')
  const [providerName, setProviderName] = useState('custom')
  const [importing, setImporting] = useState(false)

  // Парсинг нод и профилей csqtt:// в реальном времени при вводе текста
  const csqttLines = useMemo(() => {
    return inputText
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l.toLowerCase().startsWith('csqtt://'))
  }, [inputText])

  const parsedCsqtt = useMemo(() => {
    return csqttLines.map((raw) => ({ raw, parsed: parseCsqttLink(raw) }))
  }, [csqttLines])

  const parsedNodes: ParsedNode[] = useMemo(() => {
    return parseMultipleLinks(inputText)
  }, [inputText])

  const combinedYaml = useMemo(() => {
    return parsedNodes.map((n) => n.yaml).join('\n\n')
  }, [parsedNodes])

  const totalFound = parsedNodes.length + parsedCsqtt.length

  const handleCopyYaml = async () => {
    if (!combinedYaml) {
      notify('Нет распознанных нод для копирования', true)
      return
    }
    const ok = await copyToClipboard(combinedYaml)
    if (ok) {
      notify(`Скопирован YAML для ${parsedNodes.length} нод(ы)`)
    } else {
      notify('Не удалось скопировать в буфер обмена', true)
    }
  }

  const handleImport = async () => {
    if (totalFound === 0) {
      notify('Вставьте хотя бы одну корректную ссылку на сервер', true)
      return
    }

    setImporting(true)
    try {
      if (csqttLines.length > 0) {
        let profile = parseCsqttLink(csqttLines[0])
        if (!profile) {
          const dec = await decryptCsqttProfile(csqttLines[0])
          profile = parseCsqttLink(dec.link)
        }
        if (!profile) {
          notify('Не удалось распознать профиль VK-туннеля csqtt://', true)
          return
        }
        await saveVkTurnConfig({
          enabled: true,
          peer: `${profile.host}:${profile.port}`,
          password: profile.password,
          ...(profile.hashes && profile.hashes.length > 0
            ? {
                links: profile.hashes.map((h) => `https://vk.com/call/join/${h}`),
                auth_mode: 'manual',
              }
            : {}),
          ...(profile.token
            ? { profile_token: profile.token, auth_mode: 'profile_token' }
            : {}),
          ...(profile.power ? { streams: profile.power } : {}),
        })
        notify(`✓ Профиль VK-туннеля (${profile.host}:${profile.port}) успешно импортирован`)
        if (onImportSuccess) onImportSuccess()
        onClose()
        window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'vkturn' }))
        return
      }

      const res = await apiPost<{ imported: boolean; target: string; file?: string }>('servers/import-node', {
        yaml_content: combinedYaml,
        target,
        provider_name: target === 'provider' ? providerName : undefined,
      })

      if (res.imported) {
        notify(`✓ Успешно импортировано ${parsedNodes.length} нод(ы) в ${target === 'config' ? 'config.yaml' : providerName + '.yaml'}`)
        if (onImportSuccess) onImportSuccess()
        onClose()
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err)
      notify('Ошибка импорта: ' + msg, true)
    } finally {
      setImporting(false)
    }
  }

  if (!isOpen) return null

  return (
    <div className="modal-backdrop">
      <div className="modal-card generator-card">
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">📥</span>
            <h2>Импорт серверов из ссылок (Outbound Generator)</h2>
          </div>
          <button className="btn btn-sm" onClick={onClose}>
            ✕
          </button>
        </div>

        <p className="muted" style={{ margin: '4px 0 16px' }}>
          Поддерживаются форматы: <code>vless://</code>, <code>vmess://</code>, <code>ss://</code>, <code>trojan://</code>, <code>hysteria2://</code>, <code>tuic://</code> и профили VK-туннеля <code>csqtt://</code>.
        </p>

        <div className="generator-body">
          <div className="form-group">
            <label className="form-label">
              <span>Ссылки на серверы (по одной на строку):</span>
              {totalFound > 0 && (
                <span className="badge badge-success">
                  Найдено: {totalFound}
                </span>
              )}
            </label>
            <textarea
              className="input-text generator-textarea"
              placeholder="vless://uuid@host:443?security=reality&pbk=...#MyServer&#10;csqtt://connect?v=2&host=..."
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              rows={5}
              autoFocus
            />
          </div>

          {parsedCsqtt.length > 0 && (
            <div className="parsed-nodes-section">
              <label className="form-label">Профиль VK-туннеля (csqtt://):</label>
              <div className="parsed-nodes-list">
                {parsedCsqtt.map((item, idx) => (
                  <div key={idx} className="parsed-node-item" style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', padding: '6px 0' }}>
                    <span className="node-proto-badge">VK-CALL</span>
                    <span className="node-name" style={{ fontWeight: 600 }}>Профиль VK-туннеля (csqtt)</span>
                    <span className="node-host muted">
                      {item.parsed ? `${item.parsed.host}:${item.parsed.port}` : 'Зашифрованный профиль (расшифруется при импорте)'}
                    </span>
                  </div>
                ))}
              </div>
              <p className="muted small" style={{ marginTop: 8 }}>
                После импорта профиль сразу сохранится и откроется вкладка «📞 Звонки ВКонтакте».
              </p>
            </div>
          )}

          {parsedNodes.length > 0 && (
            <div className="parsed-nodes-section">
              <label className="form-label">Распознанные серверы ({parsedNodes.length}):</label>
              <div className="parsed-nodes-list">
                {parsedNodes.map((node, idx) => (
                  <div key={idx} className="parsed-node-item">
                    <span className="node-proto-badge">{node.protocol}</span>
                    <span className="node-name">{node.name}</span>
                    <span className="node-host muted">
                      {node.raw.server}:{node.raw.port}
                    </span>
                    {node.raw.tls && <span className="node-feature-badge">TLS</span>}
                    {node.raw['reality-opts'] && <span className="node-feature-badge reality">Reality</span>}
                  </div>
                ))}
              </div>

              <div className="form-group" style={{ marginTop: '14px' }}>
                <label className="form-label">Предварительный просмотр YAML:</label>
                <pre className="yaml-preview">
                  <code>{combinedYaml}</code>
                </pre>
              </div>

              <div className="generator-options">
                <div className="target-select-group">
                  <label className="radio-label">
                    <input
                      type="radio"
                      name="target"
                      value="config"
                      checked={target === 'config'}
                      onChange={() => setTarget('config')}
                    />
                    <span>Добавить в секцию <code>proxies:</code> основного <code>config.yaml</code></span>
                  </label>
                  <label className="radio-label">
                    <input
                      type="radio"
                      name="target"
                      value="provider"
                      checked={target === 'provider'}
                      onChange={() => setTarget('provider')}
                    />
                    <span>Сохранить в файл подписки/провайдера (<code>/opt/etc/mihomo/providers/{providerName}.yaml</code>)</span>
                  </label>
                </div>

                {target === 'provider' && (
                  <div className="provider-name-input">
                    <label>Имя провайдера:</label>
                    <input
                      type="text"
                      className="input-text"
                      value={providerName}
                      onChange={(e) => setProviderName(e.target.value.replace(/[^a-zA-Z0-9_-]/g, ''))}
                      placeholder="custom"
                    />
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        <div className="modal-footer">
          <button className="btn btn-secondary" onClick={handleCopyYaml} disabled={parsedNodes.length === 0}>
            📋 Скопировать YAML
          </button>
          <div style={{ display: 'flex', gap: '8px' }}>
            <button className="btn btn-secondary" onClick={onClose}>
              Отмена
            </button>
            <button
              className="btn btn-primary"
              disabled={totalFound === 0 || importing}
              onClick={handleImport}
            >
              {importing ? '⏳ Импорт…' : `⚡ Импортировать (${totalFound})`}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
