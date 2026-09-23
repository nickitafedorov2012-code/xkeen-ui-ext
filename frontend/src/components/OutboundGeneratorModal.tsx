import { useState, useMemo } from 'react'
import { parseMultipleLinks, type ParsedNode } from '../utils/nodeParser'
import { apiPost } from '../api'

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
  if (!isOpen) return null
  const [inputText, setInputText] = useState('')
  const [target, setTarget] = useState<'config' | 'provider'>('config')
  const [providerName, setProviderName] = useState('custom')
  const [importing, setImporting] = useState(false)

  // Парсинг нод в реальном времени при вводе текста
  const parsedNodes: ParsedNode[] = useMemo(() => {
    return parseMultipleLinks(inputText)
  }, [inputText])

  const combinedYaml = useMemo(() => {
    return parsedNodes.map((n) => n.yaml).join('\n\n')
  }, [parsedNodes])

  const handleCopyYaml = () => {
    if (!combinedYaml) {
      notify('Нет распознанных нод для копирования', true)
      return
    }
    navigator.clipboard.writeText(combinedYaml)
    notify(`Скопирован YAML для ${parsedNodes.length} нод(ы)`)
  }

  const handleImport = async () => {
    if (parsedNodes.length === 0) {
      notify('Вставьте хотя бы одну корректную ссылку на сервер', true)
      return
    }

    setImporting(true)
    try {
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
    } catch (err: any) {
      notify('Ошибка импорта: ' + err.message, true)
    } finally {
      setImporting(false)
    }
  }

  return (
    <div className="modal-backdrop">
      <div className="modal-card generator-card">
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">🪄</span>
            <h2>Импорт серверов из ссылок (Outbound Generator)</h2>
          </div>
          <button className="btn btn-sm" onClick={onClose}>
            ✕
          </button>
        </div>

        <p className="muted" style={{ margin: '4px 0 16px' }}>
          Поддерживаются форматы: <code>vless:// (Reality/gRPC/WS)</code>, <code>vmess://</code>, <code>ss://</code>, <code>trojan://</code>, <code>hysteria2://</code>, <code>tuic://</code>. Можно вставить несколько ссылок сразу.
        </p>

        <div className="generator-body">
          <div className="form-group">
            <label className="form-label">
              <span>Ссылки на серверы (по одной на строку):</span>
              {parsedNodes.length > 0 && (
                <span className="badge badge-success">
                  Найдено: {parsedNodes.length}
                </span>
              )}
            </label>
            <textarea
              className="input-text generator-textarea"
              placeholder="vless://uuid@host:443?security=reality&pbk=...#MyServer&#10;ss://base64...#SecondServer"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              rows={5}
              autoFocus
            />
          </div>

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
              disabled={parsedNodes.length === 0 || importing}
              onClick={handleImport}
            >
              {importing ? '⏳ Импорт…' : `⚡ Импортировать (${parsedNodes.length})`}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
