// ==UserScript==
// @name         Flow Unlocker (RU) — nickitafedorov2012
// @namespace    https://github.com/nickitafedorov2012-code/xkeen-ui-ext
// @version      2.1.0
// @description  Разблокировка Google Flow (flow.google.com) на русском языке: обход регионального фильтра cPZSdc, установка русской локали и подавление редиректа на unsupported-country
// @author       nickitafedorov2012-code
// @match        https://flow.google.com/*
// @match        http://flow.google.com/*
// @run-at       document-start
// @grant        none
// ==/UserScript==

(function() {
  'use strict';

  console.info('[Flow Unlocker by nickitafedorov2012] Инициализация хуков обхода региональных ограничений и русификации...');

  // 1. Установка русского языка интерфейса и локали (ru-RU)
  try {
    Object.defineProperty(navigator, 'language', { get: () => 'ru-RU', configurable: true });
    Object.defineProperty(navigator, 'languages', { get: () => ['ru-RU', 'ru', 'en-US', 'en'], configurable: true });

    if (window.Intl && Intl.DateTimeFormat) {
      const origResolved = Intl.DateTimeFormat.prototype.resolvedOptions;
      Intl.DateTimeFormat.prototype.resolvedOptions = function() {
        const opts = origResolved.call(this);
        opts.locale = 'ru-RU';
        return opts;
      };
    }

    // Если в URL указан другой язык или параметр hl отсутствует, переключаем на русский hl=ru
    const curUrl = new URL(window.location.href);
    if (curUrl.searchParams.get('hl') !== 'ru' && !window.location.pathname.includes('unsupported-country')) {
      curUrl.searchParams.set('hl', 'ru');
      window.history.replaceState(null, '', curUrl.pathname + curUrl.search + curUrl.hash);
    }
  } catch (e) {
    console.warn('[Flow Unlocker by nickitafedorov2012] Ошибка настройки локали:', e);
  }

  // 2. Блокировка клиентского редиректа на /unsupported-country
  const BLOCKED_PATH = 'unsupported-country';
  try {
    const origPushState = history.pushState;
    history.pushState = function(state, title, url) {
      if (url && String(url).includes(BLOCKED_PATH)) {
        console.warn('[Flow Unlocker by nickitafedorov2012] Заблокирован pushState на:', url);
        return;
      }
      return origPushState.apply(this, arguments);
    };

    const origReplaceState = history.replaceState;
    history.replaceState = function(state, title, url) {
      if (url && String(url).includes(BLOCKED_PATH)) {
        console.warn('[Flow Unlocker by nickitafedorov2012] Заблокирован replaceState на:', url);
        return;
      }
      return origReplaceState.apply(this, arguments);
    };

    if (window.navigation) {
      window.navigation.addEventListener('navigate', (e) => {
        if (e.destination && e.destination.url && e.destination.url.includes(BLOCKED_PATH)) {
          console.warn('[Flow Unlocker by nickitafedorov2012] Заблокирована навигация на:', e.destination.url);
          e.preventDefault();
        }
      });
    }
  } catch (e) {
    console.warn('[Flow Unlocker by nickitafedorov2012] Ошибка перехвата History API:', e);
  }

  // 3. Модификация внутреннего батчевого RPC-протокола Google (batchexecute?rpcids=cPZSdc)
  function patchGoogleBatchExecute(responseText) {
    const lines = responseText.split('\n');
    let beforeValue = null;
    let modifiedCount = 0;

    for (let i = 0; i < lines.length; i++) {
      if (!lines[i].startsWith('[[')) continue;

      let parsed;
      try {
        parsed = JSON.parse(lines[i]);
      } catch {
        continue;
      }

      let lineModified = false;
      for (const item of parsed) {
        // item: ["wrb.fr", null, "[\"US\", false, ...]"]
        if (!Array.isArray(item) || item[0] !== 'wrb.fr' || item[1] !== null || typeof item[2] !== 'string') {
          continue;
        }

        let config;
        try {
          config = JSON.parse(item[2]);
        } catch {
          continue;
        }

        if (!Array.isArray(config) || config.length < 2) continue;

        beforeValue = config[1];
        config[1] = true; // ПРИНУДИТЕЛЬНО ВКЛЮЧАЕМ ДОСТУПНОСТЬ (isSupported = true)
        if (config[0] === 'RU' || !config[0]) {
          config[0] = 'US';
        }
        item[2] = JSON.stringify(config);
        lineModified = true;
        modifiedCount++;
      }

      if (lineModified) {
        const cleanLine = lines[i].replace(/\r$/, '');
        const originalLen = Number(lines[i - 1]);

        const utf8ByteLen = (str) => new TextEncoder().encode(str).length;
        const strCharLen = (str) => str.length;

        const lenCalc = [utf8ByteLen, strCharLen].find(calc =>
          [0, 1, 2].includes(originalLen - calc(cleanLine))
        );

        if (lenCalc) {
          const newLine = JSON.stringify(parsed);
          lines[i - 1] = String(originalLen + (lenCalc(newLine) - lenCalc(cleanLine)));
          lines[i] = newLine;
        }
      }
    }

    if (modifiedCount === 0) {
      throw new Error('RPC config payload not found or already patched');
    }

    return {
      body: lines.join('\n'),
      before: beforeValue
    };
  }

  // 4. Перехват XMLHttpRequest (batchexecute в Angular)
  const targetXHRs = new WeakMap();
  const patchedBodies = new WeakMap();

  const origOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url, ...args) {
    let isTarget = false;
    try {
      const fullUrl = new URL(url, location.href);
      isTarget = fullUrl.pathname.includes('/_/AiSandboxAngularFrontend/data/batchexecute')
              && (fullUrl.searchParams.get('rpcids')?.includes('cPZSdc') || fullUrl.href.includes('cPZSdc'));
    } catch {}

    targetXHRs.set(this, isTarget);
    patchedBodies.delete(this);
    return origOpen.apply(this, [method, url, ...args]);
  };

  for (const prop of ['responseText', 'response']) {
    const desc = Object.getOwnPropertyDescriptor(XMLHttpRequest.prototype, prop);
    if (!desc || !desc.get) continue;

    Object.defineProperty(XMLHttpRequest.prototype, prop, {
      ...desc,
      get() {
        const origVal = desc.get.call(this);
        if (!targetXHRs.get(this) || typeof origVal !== 'string') {
          return origVal;
        }

        if (this.readyState !== 4) {
          return origVal;
        }

        if (!patchedBodies.has(this)) {
          try {
            const res = patchGoogleBatchExecute(origVal);
            patchedBodies.set(this, res.body);
            console.info('[Flow Unlocker by nickitafedorov2012] ✓ Успешно подменен ответ RPC cPZSdc (ранее:', res.before, '-> теперь: true)');
            document.documentElement.setAttribute('data-flow-unlock', 'applied');
            document.documentElement.setAttribute('data-xkeen-flow-unlock', 'applied');
          } catch (e) {
            patchedBodies.set(this, origVal);
            console.debug('[Flow Unlocker by nickitafedorov2012] Без изменений:', e.message);
          }
        }

        return patchedBodies.get(this);
      }
    });
  }

  // 5. Перехват fetch (для сопутствующих API boq-labs / aisandbox)
  try {
    const origFetch = window.fetch;
    window.fetch = function(input, init) {
      const url = (typeof input === 'string') ? input : (input?.url || '');
      return origFetch.apply(this, arguments).then(response => {
        if (response.url && response.url.includes(BLOCKED_PATH)) {
          console.warn('[Flow Unlocker by nickitafedorov2012] Заблокирован fetch редирект на:', response.url);
          return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } });
        }

        if (url.includes('boq-labs') || url.includes('aisandbox') || url.includes('country') || url.includes('eligib')) {
          const clone = response.clone();
          return clone.text().then(text => {
            if (text.includes('"isSupported":false') || text.includes('"eligible":false') || text.includes('"RU"')) {
              console.info('[Flow Unlocker by nickitafedorov2012] Модификация JSON fetch ответа для:', url);
              const modified = text
                .replace(/"isSupported"\s*:\s*false/gi, '"isSupported":true')
                .replace(/"eligible"\s*:\s*false/gi, '"eligible":true')
                .replace(/"available"\s*:\s*false/gi, '"available":true')
                .replace(/"RU"/g, '"US"');
              return new Response(modified, {
                status: response.status,
                statusText: response.statusText,
                headers: response.headers
              });
            }
            return response;
          }).catch(() => response);
        }

        return response;
      });
    };
  } catch (e) {
    console.warn('[Flow Unlocker by nickitafedorov2012] Ошибка перехвата fetch:', e);
  }

  // 6. Если страница уже открылась на /unsupported-country — возвращаем на главную на русском языке
  if (location.pathname.includes(BLOCKED_PATH)) {
    console.info('[Flow Unlocker by nickitafedorov2012] Текущий URL unsupported-country, возврат на главную (ru)...');
    history.replaceState(null, '', '/?authuser=0&hl=ru');
    location.replace('https://flow.google.com/?authuser=0&hl=ru');
  }

  console.info('[Flow Unlocker by nickitafedorov2012] Все хуки активны. Проверка региона отключена, русский язык включен.');
})();
