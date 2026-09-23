export interface PresetService {
  id: string
  name: string
  icon: string
  description: string
  domains: string[]
}

export interface PresetCategory {
  id: string
  name: string
  icon: string
  services: PresetService[]
}

export const PRESET_CATEGORIES: PresetCategory[] = [
  {
    id: 'ai',
    name: 'Искусственный интеллект & LLM',
    icon: '🤖',
    services: [
      {
        id: 'chatgpt',
        name: 'ChatGPT / OpenAI',
        icon: '🟢',
        description: 'ChatGPT, API OpenAI, генератор изображений DALL-E',
        domains: ['chatgpt.com', 'openai.com', 'oaistatic.com', 'oaiusercontent.com', 'auth0.openai.com'],
      },
      {
        id: 'claude',
        name: 'Claude / Anthropic',
        icon: '🟣',
        description: 'Чат Claude 3.5 Sonnet / Opus и веб-портал Anthropic',
        domains: ['claude.ai', 'anthropic.com'],
      },
      {
        id: 'gemini',
        name: 'Google Gemini & AI Studio',
        icon: '✨',
        description: 'Gemini Web, Google AI Studio, MakerSuite API',
        domains: ['gemini.google.com', 'aistudio.google.com', 'makersuite.google.com', 'deepmind.google', 'flow.google.com'],
      },
      {
        id: 'copilot',
        name: 'Microsoft Copilot',
        icon: '🔷',
        description: 'Microsoft Copilot и сопутствующие сервисы аутентификации',
        domains: ['copilot.microsoft.com', 'bing.com', 'sydney.bing.com'],
      },
      {
        id: 'midjourney',
        name: 'Midjourney',
        icon: '🎨',
        description: 'Веб-генератор Midjourney и галерея',
        domains: ['midjourney.com'],
      },
    ],
  },
  {
    id: 'media',
    name: 'Стриминг & Медиа',
    icon: '🎬',
    services: [
      {
        id: 'youtube',
        name: 'YouTube',
        icon: '▶️',
        description: 'Видеосерверы YouTube (Googlevideo CDN, превью, стримы)',
        domains: ['youtube.com', 'googlevideo.com', 'ytimg.com', 'youtu.be', 'youtubei.googleapis.com'],
      },
      {
        id: 'netflix',
        name: 'Netflix',
        icon: '🍿',
        description: 'Фильмы, сериалы и видео-CDN Netflix',
        domains: ['netflix.com', 'nflxvideo.net', 'nflxext.com', 'nflximg.net'],
      },
      {
        id: 'spotify',
        name: 'Spotify',
        icon: '🎵',
        description: 'Музыкальный стриминг и веб-плеер Spotify',
        domains: ['spotify.com', 'scdn.co', 'spoti.fi'],
      },
      {
        id: 'twitch',
        name: 'Twitch',
        icon: '👾',
        description: 'Прямые трансляции Twitch и стриминг-серверы',
        domains: ['twitch.tv', 'ttvnw.net', 'jtvnw.net'],
      },
      {
        id: 'soundcloud',
        name: 'SoundCloud',
        icon: '☁️',
        description: 'Аудиохостинг SoundCloud и медиа-CDN',
        domains: ['soundcloud.com', 'sndcdn.com'],
      },
    ],
  },
  {
    id: 'gaming',
    name: 'Игры & Коммуникации',
    icon: '🎮',
    services: [
      {
        id: 'discord',
        name: 'Discord',
        icon: '💬',
        description: 'Голосовые каналы Discord, текст, медиа и шлюзы',
        domains: ['discord.com', 'discordapp.com', 'discord.gg', 'discordapp.net', 'discord.media'],
      },
      {
        id: 'steam',
        name: 'Steam Community',
        icon: '🚂',
        description: 'Сообщество Steam, мастерская, профили и торговая площадка',
        domains: ['steamcommunity.com', 'steampowered.com', 'steamstatic.com'],
      },
      {
        id: 'epicgames',
        name: 'Epic Games Store',
        icon: '⚡',
        description: 'Магазин Epic Games и лаунчер',
        domains: ['epicgames.com', 'unrealengine.com'],
      },
      {
        id: 'playstation',
        name: 'PlayStation Network',
        icon: '🎮',
        description: 'PSN магазин и сетевые службы Sony',
        domains: ['playstation.com', 'sonyentertainmentnetwork.com', 'playstation.net'],
      },
    ],
  },
  {
    id: 'censorship',
    name: 'Популярные ресурсы & Трекеры',
    icon: '🌐',
    services: [
      {
        id: 'rutracker',
        name: 'RuTracker',
        icon: '📦',
        description: 'Крупнейший торрент-трекер RuTracker и постеры',
        domains: ['rutracker.org', 'rutracker.net', 'rutracker.cc'],
      },
      {
        id: 'ntc',
        name: 'NTC Party',
        icon: '🛡️',
        description: 'Форум NTC.party (новости блокировок и обхода ТСПУ)',
        domains: ['ntc.party'],
      },
      {
        id: 'flibusta',
        name: 'Флибуста',
        icon: '📚',
        description: 'Книжное братство Флибуста',
        domains: ['flibusta.is', 'flibusta.site'],
      },
      {
        id: 'kinozal',
        name: 'Кинозал ТВ',
        icon: '🎥',
        description: 'Трекер Кинозал.ТВ',
        domains: ['kinozal.tv', 'kinozal.me'],
      },
      {
        id: 'rutor',
        name: 'Rutor',
        icon: '🧲',
        description: 'Открытый трекер Rutor',
        domains: ['rutor.info', 'rutor.is'],
      },
      {
        id: 'twitter',
        name: 'Twitter / X',
        icon: '🐦',
        description: 'Социальная сеть X (Twitter), картинки и медиа',
        domains: ['x.com', 'twitter.com', 'twimg.com', 't.co'],
      },
      {
        id: 'instagram',
        name: 'Instagram',
        icon: '📸',
        description: 'Instagram фото, видео и CDN-серверы',
        domains: ['instagram.com', 'cdninstagram.com'],
      },
    ],
  },
]
