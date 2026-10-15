// A fake browser just big enough for visits.js: the script runs as the shipped
// IIFE inside a vm context whose globals are these objects. `sent` records every
// beacon or fetch. `fire(target, type, event)` dispatches to listeners.
import { readFileSync } from "node:fs";
import vm from "node:vm";

const SRC = (name) => readFileSync(new URL(`../src/${name}.js`, import.meta.url), "utf8");

class Target {
  constructor() { this.listeners = {}; }
  addEventListener(type, fn) { (this.listeners[type] ??= []).push(fn); }
  removeEventListener(type, fn) { this.listeners[type] = (this.listeners[type] ?? []).filter((f) => f !== fn); }
}

export function load(options = {}) {
  const {
    href = "https://example.com/research/a-doc?utm_source=news&token=secret",
    hostname = "example.com",
    referrer = "https://www.google.com/",
    title = "A doc",
    canonical = null,            // an href, or null for no canonical link
    visibility = "visible",
    dataset = { visits: "/v" },  // the script tag's data-* as camelCased keys
    beaconReturns = true,
    readEnd = false,             // whether a [data-read-end] element exists
    storage = {},                // the fake localStorage's contents
    webdriver = false,
    scripts = ["visits"],
    framed = false,
    storageThrows = false,
    noPerformance = false,
    noBeacon = false,
  } = options;

  const sent = [];
  const url = new URL(href);
  const doc = new Target();
  const win = new Target();
  const readEndEl = readEnd ? { tag: "read-end" } : null;
  const observers = [];
  const elements = {
    "script[data-visits]": { dataset },
    "link[rel=canonical][href]": canonical ? { href: canonical } : null,
    "[data-read-end]": readEndEl,
  };
  Object.assign(doc, {
    title, referrer, visibilityState: visibility,
    documentElement: { scrollHeight: 4000 },
    querySelector: (sel) => elements[sel] ?? null,
  });
  const location = { href, hostname, host: url.host, pathname: url.pathname, search: url.search, hash: url.hash, protocol: url.protocol };
  const navigator = {
    webdriver, language: "en-US", maxTouchPoints: 0,
    sendBeacon: (u, body) => { sent.push({ method: "beacon", url: u, body }); return beaconReturns; },
  };
  const fetch = (u, init = {}) => { sent.push({ method: init.method ?? "GET", url: u, body: init.body, keepalive: init.keepalive }); return Promise.resolve({}); };
  const ctx = {
    document: doc, navigator, location, fetch, sent,
    screen: { width: 1512, height: 982 }, innerWidth: 1200, innerHeight: 800, devicePixelRatio: 2, scrollY: 0,
    matchMedia: (q) => ({ matches: q.includes("dark") }),
    Intl: { DateTimeFormat: () => ({ resolvedOptions: () => ({ timeZone: "America/New_York" }) }) },
    performance: { interactionCount: 0, getEntriesByType: (t) => (t === "navigation" ? [{ type: "navigate", responseStart: 180 }] : []) },
    localStorage: storage,
    IntersectionObserver: class { constructor(cb) { this.cb = cb; observers.push(this); } observe() {} disconnect() { this.gone = true; } },
    PerformanceObserver: class {
      constructor(cb) { this.cb = cb; }
      observe(opts) { (ctx.perfObservers ??= []).push({ type: opts.type, opts, cb: this.cb }); }
    },
    perfObservers: [],
    Math, JSON, Object, Date, URL, URLSearchParams, console,
    addEventListener: (t, f) => win.addEventListener(t, f),
    removeEventListener: (t, f) => win.removeEventListener(t, f),
  };
  if (storageThrows) Object.defineProperty(ctx, "localStorage", { get() { throw new Error("blocked"); } });
  if (noPerformance) delete ctx.performance;
  if (noBeacon) delete navigator.sendBeacon;
  ctx.window = ctx;
  ctx.parent = framed ? {} : ctx;
  ctx.self = ctx;
  vm.createContext(ctx);
  for (const s of scripts) vm.runInContext(SRC(s), ctx, { filename: `${s}.js` });

  const fire = (target, type, event = {}) => {
    const t = target === "document" ? doc : win;
    for (const fn of [...(t.listeners[type] ?? [])]) fn(event);
  };
  const intersect = () => { for (const o of observers) o.cb([{ isIntersecting: true }], o); };
  return { sent, ctx, doc, fire, intersect };
}

export const last = (sent) => JSON.parse(sent.at(-1).body);

export const emit = (ctx, type, entries) => {
  for (const o of ctx.perfObservers) if (o.type === type) o.cb({ getEntries: () => entries });
};
