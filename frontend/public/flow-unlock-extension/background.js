const SCRIPT_ID = 'flow-unlock-hook';

async function registerHook() {
  try {
    const existing = await chrome.scripting.getRegisteredContentScripts({ ids: [SCRIPT_ID] });
    if (existing.length === 0) {
      await chrome.scripting.registerContentScripts([{
        id: SCRIPT_ID,
        matches: ["https://flow.google.com/*", "http://flow.google.com/*"],
        js: ["hook.js"],
        runAt: "document_start",
        world: "MAIN",
        persistAcrossSessions: true
      }]);
      console.info('[Flow Unlocker by nickitafedorov2012] Хук успешно зарегистрирован в MAIN world');
    }
  } catch (e) {
    console.warn('[Flow Unlocker by nickitafedorov2012] Регистрация хука:', e.message);
  }
}

chrome.runtime.onInstalled.addListener(() => {
  registerHook();
});

chrome.runtime.onStartup.addListener(() => {
  registerHook();
});
