/**
 * Flow Unlocker — Popup Controller
 * Автор: nickitafedorov2012
 * GitHub: https://github.com/nickitafedorov2012-code
 */

const toggle = document.getElementById('enabled');
const statusText = document.getElementById('status');
const statusDot = document.getElementById('status-dot');
const error = document.getElementById('error');
const reload = document.getElementById('reload');
const openBtn = document.getElementById('open');
let currentTab;

function setStatus(text, state = 'active') {
  if (statusText) {
    statusText.textContent = text;
  }
  if (statusDot) {
    statusDot.className = 'dot ' + state;
  }
}

function setError(msg) {
  if (!error) return;
  if (msg) {
    error.textContent = msg;
    error.hidden = false;
  } else {
    error.textContent = '';
    error.hidden = true;
  }
}

async function init() {
  try {
    // 1. Проверяем регистрацию динамического контентного скрипта
    const registered = await chrome.scripting.getRegisteredContentScripts({ ids: ['flow-helper'] });
    const isRegistered = Boolean(registered && registered.length > 0);
    toggle.checked = isRegistered;

    // 2. Получаем текущую активную вкладку
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
    currentTab = tabs[0];
    const isFlowTab = Boolean(currentTab?.url?.startsWith('https://flow.google.com/'));

    if (reload) {
      reload.hidden = !isFlowTab;
    }

    if (!toggle.checked) {
      setStatus('Отключено. Включите тумблер для активации.', 'inactive');
    } else {
      setStatus('Обход включён. Готово к работе с Flow.', 'active');
    }

    // 3. Если вкладка Google Flow открыта и обход включен — опрашиваем status.js
    if (isFlowTab && toggle.checked && currentTab?.id) {
      try {
        const res = await chrome.tabs.sendMessage(currentTab.id, { type: 'status' });
        if (res?.active || res?.applied) {
          setStatus('Обход активен! Региональный фильтр снят.', 'active');
        } else if (res?.state === 'ready') {
          setStatus('Готово. Ожидание первого ответа Flow…', 'active');
        } else if (res?.state && res.state.startsWith('schema mismatch')) {
          setStatus('Внимание: возможно, изменилась структура ответов Flow.', 'warning');
        } else {
          setStatus('Для активации обхода перезагрузите эту вкладку Flow.', 'loading');
        }
      } catch {
        setStatus('Для активации обхода перезагрузите эту вкладку Flow.', 'loading');
      }
    }
  } catch (err) {
    setError(err?.message || 'Ошибка инициализации расширения');
    setStatus('Ошибка инициализации', 'error');
  } finally {
    toggle.disabled = false;
  }
}

// Переключение тумблера включения / выключения
toggle.addEventListener('change', async () => {
  toggle.disabled = true;
  setError('');
  setStatus('Применение настроек…', 'loading');

  try {
    const res = await chrome.runtime.sendMessage({
      type: 'setEnabled',
      enabled: toggle.checked
    });

    if (!res?.ok) {
      throw new Error(res?.error || 'Не удалось применить настройки');
    }

    if (toggle.checked) {
      setStatus('Обход включён! Перезагрузите открытые вкладки Flow.', 'active');
    } else {
      setStatus('Обход отключён.', 'inactive');
    }
  } catch (err) {
    toggle.checked = !toggle.checked;
    setError(err?.message || 'Не удалось изменить состояние обхода');
    setStatus('Ошибка применения', 'error');
  } finally {
    toggle.disabled = false;
  }
});

// Кнопка открытия Google Flow
if (openBtn) {
  openBtn.onclick = () => {
    chrome.tabs.create({ url: 'https://flow.google.com/?authuser=0&hl=en' });
  };
}

// Кнопка перезагрузки текущей вкладки Flow
if (reload) {
  reload.onclick = async () => {
    setError('');
    try {
      const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
      const targetTab = tabs[0];
      if (!targetTab?.id || !targetTab.url?.startsWith('https://flow.google.com/')) {
        throw new Error('Сначала выберите открытую вкладку Google Flow');
      }

      const targetUrl = new URL(targetTab.url);
      if (targetUrl.pathname.endsWith('/unsupported-country')) {
        targetUrl.pathname = targetUrl.pathname.replace(/\/unsupported-country$/, '') || '/';
        await chrome.tabs.update(targetTab.id, { url: targetUrl.href });
      } else {
        await chrome.tabs.reload(targetTab.id);
      }
      window.close();
    } catch (err) {
      setError(err?.message || 'Ошибка обновления вкладки');
    }
  };
}

init().catch((err) => {
  setError(err?.message || 'Ошибка запуска интерфейса');
  setStatus('Ошибка', 'error');
});