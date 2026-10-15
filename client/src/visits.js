(() => {
  var d = document, w = window, n = navigator, l = location, sc = d.querySelector('script[data-visits]');
  if (!sc || w !== w.parent) return;
  var ds = sc.dataset, ep = ds.visits, get = ds.visitsMethod == 'get', h = l.hash;
  try {
    var ls = w.localStorage;
    if (h == '#visits-off') { ls.visits = 'off'; return }
    if (h == '#visits-on') delete ls.visits;
    if (ls.visits == 'off') return;
  } catch (e) {}
  if (!('visitsLocal' in ds) && (l.protocol == 'file:' || /^(localhost$|127\.|10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.|0\.0\.0\.0$)/.test(l.hostname))) return;

  var strip = s => s.replace(/^www\./, ''), p = l.pathname, c = d.querySelector('link[rel=canonical][href]');
  if (c) try { var u = new URL(c.href, l.href); if (/^https?:$/.test(u.protocol) && strip(u.hostname) == strip(l.hostname)) p = u.pathname } catch (e) {}
  var id = Math.random().toString(36).slice(2, 12);

  var send = (name, data) => {
    var e = Object.assign({ v: 1, n: name, id: id, p: p, ts: Date.now() }, data);
    try {
      if (get) {
        if (e.d) e.d = JSON.stringify(e.d);
        e = JSON.parse(JSON.stringify(e));
        fetch(ep + (ep.indexOf('?') < 0 ? '?' : '&') + new URLSearchParams(e), { keepalive: true, mode: 'no-cors' });
      } else {
        var j = JSON.stringify(e);
        if (!(n.sendBeacon && n.sendBeacon(ep, j))) fetch(ep, { method: 'POST', body: j, keepalive: true, mode: 'no-cors' });
      }
    } catch (e) {}
  };

  var view = () => { try {
    var q = new URLSearchParams(l.search), cp = {}, keys = { utm_source: 'us', utm_medium: 'um', utm_campaign: 'uc', utm_term: 'ut', utm_content: 'uo', ref: 'cr' }, k;
    for (k in keys) if (q.has(k)) cp[keys[k]] = q.get(k).slice(0, 100);
    var m = s => matchMedia(s).matches || undefined, nav = performance.getEntriesByType('navigation')[0] || {};
    send('view', Object.assign({
      t: d.title.slice(0, 200), r: d.referrer.split(/[?#]/)[0].slice(0, 2048), nt: nav.type,
      sw: screen.width, sh: screen.height, vw: innerWidth, vh: innerHeight, dpr: +devicePixelRatio.toFixed(2),
      th: m('(prefers-color-scheme: dark)') ? 'dark' : 'light', rm: m('(prefers-reduced-motion: reduce)'), tp: n.maxTouchPoints > 0 || undefined,
      tz: Intl.DateTimeFormat().resolvedOptions().timeZone, lg: n.language,
      b: n.webdriver ? 1 : undefined
    }, cp));
  } catch (e) {} };
  var vis = d.visibilityState == 'visible', t0 = Date.now(), eng = 0, sd = 0, re, lastJ = '';
  var depth = () => { sd = Math.max(sd, Math.min(100, Math.round((scrollY + innerHeight) / d.documentElement.scrollHeight * 100) || 0)) };
  try {
    addEventListener('scroll', depth, { passive: true }); depth();
    var end = d.querySelector('[data-read-end]');
    if (end && w.IntersectionObserver) new IntersectionObserver((es, o) => { if (es.some(x => x.isIntersecting)) { re = true; o.disconnect() } }).observe(end);
  } catch (e) {}
  var leave = () => {
    if (vis) { eng += Date.now() - t0; vis = false }
    var data = { e: Math.round(eng / 1000), sd: sd, re: re }, j = JSON.stringify(data);
    if (j != lastJ) { lastJ = j; send('leave', data) }
  };
  d.addEventListener('visibilitychange', () => { if (d.visibilityState == 'hidden') leave(); else if (!vis) { vis = true; t0 = Date.now() } });
  addEventListener('pagehide', leave);
  var click = ev => {
    var el = ev.target && ev.target.closest && ev.target.closest('a[href],[data-visits-event]');
    if (!el) return;
    var name = el.dataset.visitsEvent, out = el.host && el.host != l.host, data = {};
    if (name) data.ev = name;
    if (out) data.u = el.origin + el.pathname;
    if (name || out) send('click', data);
  };
  addEventListener('click', click, true); addEventListener('auxclick', click, true);
  w.visits = { send: (name, data) => { if (typeof name == 'string' && /^[a-z][a-z0-9_-]{0,31}$/.test(name)) send(name, { d: data }) }, id: id };
  var once = () => { if (d.visibilityState == 'visible') { d.removeEventListener('visibilitychange', once); view() } };
  d.visibilityState == 'visible' ? view() : d.addEventListener('visibilitychange', once);
})();
