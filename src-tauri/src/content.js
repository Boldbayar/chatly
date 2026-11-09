(function () {
  'use strict';

  const media = navigator.mediaDevices;
  if (media) {
    const refuse = () =>
      Promise.reject(new DOMException('Camera and microphone are turned off in Chatly.', 'NotAllowedError'));
    for (const fn of ['getUserMedia', 'getDisplayMedia']) {
      try { Object.defineProperty(media, fn, { value: refuse, writable: false, configurable: false }); } catch (_) {}
    }
  }

  const META = /(^|\.)(instagram\.com|cdninstagram\.com|facebook\.com|facebook\.net|fbcdn\.net|fbsbx\.com|fb\.com|fb\.me|messenger\.com|meta\.com|m\.me)$/i;
  document.addEventListener('click', (event) => {
    if (event.button !== 0) return;
    const link = event.target instanceof Element ? event.target.closest('a[href]') : null;
    if (!link) return;
    let url;
    try { url = new URL(link.href, location.href); } catch (_) { return; }
    if (url.protocol !== 'https:' && url.protocol !== 'http:') return;
    if (META.test(url.hostname)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    window.open(url.href, '_blank', 'noopener');
  }, true);
})();
