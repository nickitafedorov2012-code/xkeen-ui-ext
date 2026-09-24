document.getElementById('btnOpen').addEventListener('click', () => {
  chrome.tabs.create({ url: 'https://flow.google.com/?authuser=0&hl=ru' });
});

document.getElementById('btnReload').addEventListener('click', async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (tab && tab.id) {
    if (tab.url && tab.url.includes('unsupported-country')) {
      chrome.tabs.update(tab.id, { url: 'https://flow.google.com/?authuser=0&hl=ru' });
    } else {
      chrome.tabs.reload(tab.id);
    }
  }
});
