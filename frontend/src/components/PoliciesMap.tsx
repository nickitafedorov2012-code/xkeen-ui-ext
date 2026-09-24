import { useEffect, useState } from 'react'
import { apiGet } from '../api'

interface DeviceNode {
  ip: string
  mac: string
  name: string
  policy_id: string
  xkeen_server: string
  active: boolean
}

interface PolicyNode {
  id: string
  name: string
  description?: string
  is_main?: boolean
}

interface ServerNode {
  id: string
  name: string
  is_active: boolean
  ping_ms?: number
  protocol?: string
}

interface PoliciesMapResponse {
  devices: DeviceNode[]
  policies: PolicyNode[]
  servers: ServerNode[]
}

interface PoliciesMapProps {
  notify: (msg: string, error?: boolean) => void
}

export default function PoliciesMap({ notify }: PoliciesMapProps) {
  const [data, setData] = useState<PoliciesMapResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [selectedDevice, setSelectedDevice] = useState<DeviceNode | null>(null)

  const loadData = async () => {
    setLoading(true)
    try {
      const res = await apiGet<PoliciesMapResponse>('policies/map')
      setData(res)
      if (res.devices.length > 0 && !selectedDevice) {
        setSelectedDevice(res.devices[0])
      }
    } catch (e: any) {
      notify('Ошибка загрузки карты политик: ' + e.message, true)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadData()
  }, [])

  if (loading || !data) {
    return <div className="card loading-placeholder">⏳ Построение карты политик роутера…</div>
  }

  const activeServerName = selectedDevice?.xkeen_server || 'По умолчанию (Основной прокси)'

  return (
    <div className="policies-map-view">
      <div className="section-header">
        <div>
          <h2>🗺️ Визуальная карта маршрутизации Keenetic</h2>
          <p className="muted">
            Наглядная топология трафика: Домашнее устройство ➔ Политика KeeneticOS ➔ Назначенный сервер XKeen / DIRECT.
          </p>
        </div>
        <button className="btn btn-sm btn-secondary" onClick={loadData}>
          🔄 Обновить карту
        </button>
      </div>

      <div className="map-columns-container">
        {/* Колонка 1: Устройства */}
        <div className="map-column">
          <div className="map-column-header">
            <h4>📱 Устройства ({data.devices.length})</h4>
            <span className="muted">Кликните для выбора</span>
          </div>
          <div className="map-nodes-list">
            {data.devices.map((d) => {
              const isSelected = selectedDevice?.ip === d.ip
              return (
                <div
                  key={d.ip}
                  className={`map-node device-node ${isSelected ? 'selected' : ''}`}
                  onClick={() => setSelectedDevice(d)}
                >
                  <div className="node-title">
                    <span className="node-icon">{d.active ? '🟢' : '⚪'}</span>
                    <strong>{d.name || d.ip}</strong>
                  </div>
                  <div className="node-sub muted">
                    <code>{d.ip}</code> • {d.mac}
                  </div>
                  {d.xkeen_server && (
                    <div className="node-badge-server">🛰 {d.xkeen_server}</div>
                  )}
                </div>
              )
            })}
          </div>
        </div>

        {/* Колонка 2: Политики Keenetic */}
        <div className="map-column">
          <div className="map-column-header">
            <h4>🛡️ Политики KeeneticOS ({data.policies.length})</h4>
            <span className="muted">Шлюзы провайдеров</span>
          </div>
          <div className="map-nodes-list">
            {data.policies.map((p) => {
              const isLinked = selectedDevice?.policy_id === p.id
              return (
                <div
                  key={p.id}
                  className={`map-node policy-node ${isLinked ? 'linked' : ''}`}
                >
                  <div className="node-title">
                    <span className="node-icon">🛡️</span>
                    <strong>{p.name}</strong>
                    {p.is_main && <span className="badge-main">Main</span>}
                  </div>
                  <div className="node-sub muted">{p.description || `ID: ${p.id}`}</div>
                  {isLinked && (
                    <div className="link-indicator">
                      ✔ Назначена для {selectedDevice?.name || selectedDevice?.ip}
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        </div>

        {/* Колонка 3: Выходные узлы XKeen */}
        <div className="map-column">
          <div className="map-column-header">
            <h4>🛰️ Выходной сервер / Прокси</h4>
            <span className="muted">Mihomo Core</span>
          </div>
          <div className="map-nodes-list">
            <div className={`map-node server-node ${selectedDevice?.xkeen_server ? 'active-target' : ''}`}>
              <div className="node-title">
                <span className="node-icon">🌐</span>
                <strong>{activeServerName}</strong>
              </div>
              <div className="node-sub muted">
                {selectedDevice ? (
                  <span>
                    Трафик <strong>{selectedDevice.name || selectedDevice.ip}</strong> выходит через этот узел.
                  </span>
                ) : (
                  'Выберите устройство'
                )}
              </div>
            </div>

            <div className="card-note">
              💡 <em>Совет:</em> Индивидуальные серверы настраиваются во вкладке <strong>«📱 Устройства»</strong>.
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
