(() => {
  var d = document, w = window, n = navigator, l = location, sc = d.querySelector('script[data-visits]');
  if (!sc || w !== w.parent) return;
  var ds = sc.dataset, ep = ds.visits, get = ds.visitsMethod == 'get', h = l.hash, ls = w.localStorage;
  try {
    if (h == '#visits-off') { ls.visits = 'off'; return }
    if (h == '#visits-on') delete ls.visits;
    if (ls.visits == 'off') return;
  } catch (e) {}
  if (!('visitsLocal' in ds) && (l.protocol == 'file:' || /^(localhost$|127\.|10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.|0\.0\.0\.0$)/.test(l.hostname))) return;

  var strip = s => s.replace(/^www\./, ''), p = l.pathname, c = d.querySelector('link[rel=canonical][href]');
  if (c) try { var u = new URL(c.href, l.href); if (/^https?:$/.test(u.protocol) && strip(u.hostname) == strip(l.hostname)) p = u.pathname } catch (e) {}
  var id = Math.random().toString(36).slice(2, 12);

  var send = (name, data) => {
    var e = Object.assign({ v: 1, n: name, id: id, p: p }, data);
    try {
      if (get) {
        if (e.d) e.d = JSON.stringify(e.d);
        fetch(ep + (ep.indexOf('?') < 0 ? '?' : '&') + new URLSearchParams(e), { keepalive: true, mode: 'no-cors' });
      } else {
        var j = JSON.stringify(e);
        if (!(n.sendBeacon && n.sendBeacon(ep, j))) fetch(ep, { method: 'POST', body: j, keepalive: true, mode: 'no-cors' });
      }
    } catch (e) {}
  };

  var view = () => {
    var q = new URLSearchParams(l.search), cp = {}, keys = { utm_source: 'us', utm_medium: 'um', utm_campaign: 'uc', utm_term: 'ut', utm_content: 'uo', ref: 'cr' }, k;
    for (k in keys) if (q.has(k)) cp[keys[k]] = q.get(k).slice(0, 100);
    var m = s => matchMedia(s).matches || undefined, nav = performance.getEntriesByType('navigation')[0] || {};
    send('view', Object.assign({
      t: d.title.slice(0, 200), r: d.referrer.slice(0, 2048), nt: nav.type,
      sw: screen.width, sh: screen.height, vw: innerWidth, vh: innerHeight, dpr: +devicePixelRatio.toFixed(2),
      th: m('(prefers-color-scheme: dark)') ? 'dark' : 'light', rm: m('(prefers-reduced-motion: reduce)'), tp: n.maxTouchPoints > 0 || undefined,
      tz: Intl.DateTimeFormat().resolvedOptions().timeZone, lg: n.language,
      b: (n.webdriver ? 1 : w.callPhantom || w._phantom ? 2 : w.__nightmare ? 3 : d.__selenium_unwrapped || d.__webdriver_evaluate ? 4 : 0) || undefined
    }, cp));
  };
  var once = () => { if (d.visibilityState == 'visible') { d.removeEventListener('visibilitychange', once); view() } };
  d.visibilityState == 'visible' ? view() : d.addEventListener('visibilitychange', once);
})();
