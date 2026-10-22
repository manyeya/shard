// Hand each new download to the Shard app, with the cookies it needs. If the app isn't running
// or turns it down, the browser keeps the download.
const APP = 'http://127.0.0.1:47110';

chrome.downloads.onCreated.addListener(async (item) => {
  const url = item.finalUrl || item.url;
  // blob: and data: URLs only exist inside the page; finished items are history being restored.
  if (!/^https?:/i.test(url) || item.state !== 'in_progress') return;
  const cookies = await chrome.cookies.getAll({ url });
  const body = new URLSearchParams({
    url,
    referrer: item.referrer || '',
    cookie: cookies.map((c) => `${c.name}=${c.value}`).join('; '),
    useragent: navigator.userAgent,
  });
  try {
    const reply = await fetch(`${APP}/add`, { method: 'POST', body });
    if (!reply.ok) return;
  } catch {
    return; // the app isn't running
  }
  await chrome.downloads.cancel(item.id);
  await chrome.downloads.erase({ id: item.id });
});

// Shard's setup waits for this to show the extension is in; a running Shard just says hello back.
chrome.runtime.onInstalled.addListener(() => {
  fetch(`${APP}/hello`, { method: 'POST' }).catch(() => {});
});
