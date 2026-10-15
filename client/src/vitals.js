(() => {
  var v = window.visits, P = window.PerformanceObserver, d = document;
  if (!v || !P) return;
  var lcp, cls = 0, win = 0, w0 = -1e9, wl = -1e9, ints = {}, done;
  var ob = (type, cb, extra) => { try { new P(l => l.getEntries().forEach(cb)).observe(Object.assign({ type: type, buffered: true }, extra)) } catch (e) {} };
  ob('largest-contentful-paint', e => lcp = e.startTime);
  ob('layout-shift', e => {
    if (e.hadRecentInput) return;
    if (e.startTime - wl > 1000 || e.startTime - w0 > 5000) { win = 0; w0 = e.startTime }
    win += e.value; wl = e.startTime; cls = Math.max(cls, win);
  });
  ob('event', e => { if (e.interactionId) ints[e.interactionId] = Math.max(ints[e.interactionId] || 0, e.duration) }, { durationThreshold: 40 });
  var fin = () => {
    if (done) return; done = 1;
    var ds = Object.keys(ints).map(k => ints[k]).sort((a, b) => b - a).slice(0, 10), c = performance.interactionCount || ds.length;
    var nav = performance.getEntriesByType('navigation')[0];
    v.send('vitals', {
      lcp: lcp && Math.round(lcp), cls: +cls.toFixed(3),
      inp: ds.length ? Math.round(ds[Math.min(Math.floor(c / 50), ds.length - 1)]) : undefined,
      ttfb: nav && Math.round(nav.responseStart)
    });
  };
  d.addEventListener('visibilitychange', () => { if (d.visibilityState == 'hidden') fin() });
  addEventListener('pagehide', fin);
})();
