'use strict';

const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);
const els = {
  grid: $('grid'), spacer: $('grid-spacer'), message: $('grid-message'),
  search: $('search'), sort: $('sort'),
  libraryButton: $('library-button'), libraryName: $('library-name'), libraryCount: $('library-count'),
  details: $('details'), detailsBody: $('details-body'), detailsClose: $('details-close'),
  empty: $('empty'), emptyMessage: $('empty-message'), chooseLibrary: $('choose-library'),
};

const isMac = /Mac/.test(navigator.userAgent);
const isWindows = /Windows/.test(navigator.userAgent);
document.documentElement.classList.toggle('is-mac', isMac);

// ---------- Layout constants ----------
const PAD = 28;          // grid padding
const GAP_X = 24;
const GAP_Y = 28;
const MIN_TILE = 128;
const MAX_TILE = 176;
const TEXT_H = 38;       // title + author lines under the cover
const OVERSCAN = 3;      // rows rendered above/below the viewport

const state = {
  library: null,
  items: [],
  indexById: new Map(),
  selectedId: null,
  cols: 1, tileW: MIN_TILE, rowH: 0, rows: 0,
  live: new Map(),       // id -> tile element currently in the DOM
  searchSeq: 0,
};

// ---------- Storage (per-device convenience only) ----------
const store = {
  get(key) { try { return localStorage.getItem(key); } catch { return null; } },
  set(key, value) { try { localStorage.setItem(key, value); } catch { /* ignore */ } },
};

// ---------- Covers ----------
function coverUrl(size, id) {
  return isWindows ? `http://cover.localhost/${size}/${id}` : `cover://localhost/${size}/${id}`;
}

function hueFor(id) {
  return (id * 47) % 360;
}

function placeholder(title, authors) {
  const ph = document.createElement('div');
  ph.className = 'placeholder';
  const t = document.createElement('div');
  t.className = 'placeholder-title';
  t.textContent = title;
  const a = document.createElement('div');
  a.className = 'placeholder-author';
  a.textContent = authors;
  ph.append(t, a);
  return ph;
}

function fillCover(container, item, size) {
  container.style.setProperty('--hue', hueFor(item.id));
  if (!item.hasCover) {
    container.append(placeholder(item.title, item.authors));
    return;
  }
  const img = new Image();
  img.decoding = 'async';
  img.alt = '';
  img.onload = () => img.classList.add('loaded');
  img.onerror = () => { img.remove(); container.append(placeholder(item.title, item.authors)); };
  img.src = coverUrl(size, item.id);
  container.append(img);
}

// ---------- Virtual grid ----------
function layout() {
  const width = els.grid.clientWidth - PAD * 2;
  let cols = Math.max(1, Math.floor((width + GAP_X) / (MIN_TILE + GAP_X)));
  let tileW = (width - GAP_X * (cols - 1)) / cols;
  while (tileW > MAX_TILE) {
    cols += 1;
    tileW = (width - GAP_X * (cols - 1)) / cols;
  }
  state.cols = cols;
  state.tileW = Math.max(60, tileW);
  state.rowH = state.tileW * 1.5 + TEXT_H + GAP_Y;
  state.rows = Math.ceil(state.items.length / cols);
  els.spacer.style.height = `${state.rows ? PAD * 2 + state.rows * state.rowH - GAP_Y : 0}px`;
  render(true);
}

function makeTile(item) {
  const tile = document.createElement('div');
  tile.className = 'tile';
  tile.dataset.id = item.id;
  tile.title = item.series ? `${item.title}\n${item.authors}\n${item.series} #${item.seriesIndex}` : `${item.title}\n${item.authors}`;

  const cover = document.createElement('div');
  cover.className = 'cover';
  fillCover(cover, item, 'thumb');
  if (item.seriesIndex) {
    const badge = document.createElement('span');
    badge.className = 'series-badge';
    badge.textContent = `#${item.seriesIndex}`;
    cover.append(badge);
  }

  const title = document.createElement('p');
  title.className = 'tile-title';
  title.textContent = item.title;
  const author = document.createElement('p');
  author.className = 'tile-author';
  author.textContent = item.authors;

  tile.append(cover, title, author);
  return tile;
}

function render(force = false) {
  const { cols, rowH, tileW, items } = state;
  if (!items.length) {
    for (const el of state.live.values()) el.remove();
    state.live.clear();
    return;
  }
  const top = els.grid.scrollTop - PAD;
  const firstRow = Math.max(0, Math.floor(top / rowH) - OVERSCAN);
  const lastRow = Math.min(state.rows - 1, Math.ceil((top + els.grid.clientHeight) / rowH) + OVERSCAN);
  const start = firstRow * cols;
  const end = Math.min(items.length, (lastRow + 1) * cols);

  const wanted = new Set();
  for (let i = start; i < end; i++) wanted.add(items[i].id);
  for (const [id, el] of state.live) {
    if (!wanted.has(id)) { el.remove(); state.live.delete(id); }
  }

  for (let i = start; i < end; i++) {
    const item = items[i];
    let el = state.live.get(item.id);
    const isNew = !el;
    if (isNew) {
      el = makeTile(item);
      state.live.set(item.id, el);
    }
    if (isNew || force) {
      const col = i % cols;
      const row = Math.floor(i / cols);
      el.style.width = `${tileW}px`;
      el.style.transform = `translate(${PAD + col * (tileW + GAP_X)}px, ${PAD + row * rowH}px)`;
      el.classList.toggle('selected', item.id === state.selectedId);
    }
    if (isNew) els.grid.append(el);
  }
}

let scrollQueued = false;
els.grid.addEventListener('scroll', () => {
  if (scrollQueued) return;
  scrollQueued = true;
  requestAnimationFrame(() => { scrollQueued = false; render(); });
}, { passive: true });

new ResizeObserver(() => layout()).observe(els.grid);

// ---------- Data ----------
async function refresh({ keepScroll = false } = {}) {
  const seq = ++state.searchSeq;
  const query = els.search.value;
  let items;
  try {
    items = await invoke('list_books', { query, sort: els.sort.value });
  } catch (err) {
    if (seq !== state.searchSeq) return;
    els.search.classList.add('invalid');
    els.search.title = String(err);
    return;
  }
  if (seq !== state.searchSeq) return; // a newer search finished first
  els.search.classList.remove('invalid');
  els.search.title = '';

  state.items = items;
  state.indexById = new Map(items.map((it, i) => [it.id, i]));
  for (const el of state.live.values()) el.remove();
  state.live.clear();
  if (!keepScroll) els.grid.scrollTop = 0;

  els.message.hidden = items.length > 0;
  els.message.textContent = query.trim() ? `No books match “${query.trim()}”.` : 'This library has no books yet.';
  const total = state.library?.bookCount ?? items.length;
  els.libraryCount.textContent = items.length === total
    ? `${total.toLocaleString()} books`
    : `${items.length.toLocaleString()} of ${total.toLocaleString()}`;
  layout();
}

async function openLibrary(path) {
  try {
    const info = await invoke('open_library', { path: path ?? null });
    state.library = info;
    store.set('libraryPath', info.path);
    els.libraryName.textContent = info.name;
    els.libraryButton.title = `${info.path}\nLoaded ${info.bookCount.toLocaleString()} books in ${info.loadMs} ms\nClick to open another library…`;
    els.empty.hidden = true;
    for (const w of info.warnings) console.warn(w);
    closeDetails();
    await refresh();
    els.grid.focus();
    return true;
  } catch (err) {
    showEmpty(String(err));
    return false;
  }
}

function showEmpty(message) {
  els.empty.hidden = false;
  if (message) els.emptyMessage.textContent = message;
}

async function chooseLibrary() {
  const path = await invoke('plugin:dialog|open', {
    options: { directory: true, multiple: false, title: 'Choose a calibre library folder' },
  });
  if (path) await openLibrary(path);
}

// ---------- Selection & details ----------
function select(id, { open = true, reveal = false } = {}) {
  state.live.get(state.selectedId)?.classList.remove('selected');
  state.selectedId = id;
  state.live.get(id)?.classList.add('selected');
  if (reveal) revealSelected();
  if (open) showDetails(id);
}

function revealSelected() {
  const i = state.indexById.get(state.selectedId);
  if (i == null) return;
  const rowTop = PAD + Math.floor(i / state.cols) * state.rowH;
  const rowBottom = rowTop + state.rowH - GAP_Y;
  const { scrollTop, clientHeight } = els.grid;
  if (rowTop < scrollTop + 8) els.grid.scrollTop = rowTop - 8;
  else if (rowBottom > scrollTop + clientHeight - 8) els.grid.scrollTop = rowBottom - clientHeight + 8;
}

let detailsSeq = 0;
async function showDetails(id) {
  const seq = ++detailsSeq;
  let b;
  try {
    b = await invoke('book_details', { id });
  } catch (err) {
    console.error(err);
    return;
  }
  if (seq !== detailsSeq) return;
  els.detailsBody.replaceChildren(renderDetails(b));
  els.details.scrollTop = 0;
  if (els.details.hidden) {
    els.details.hidden = false;
    // The grid just got narrower; re-flow and keep the selection visible.
    requestAnimationFrame(revealSelected);
  }
}

function closeDetails() {
  detailsSeq++;
  if (!els.details.hidden) {
    els.details.hidden = true;
    requestAnimationFrame(revealSelected);
  }
}

function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v);
  }
  el.append(...children.filter((c) => c != null && c !== false));
  return el;
}

function searchFor(field, value) {
  els.search.value = `${field}:"${value}"`;
  refresh();
}

const languageNames = (() => {
  try { return new Intl.DisplayNames([navigator.language, 'en'], { type: 'language' }); } catch { return null; }
})();
function languageName(code) {
  try { return languageNames?.of(code) ?? code; } catch { return code; }
}

function formatSize(bytes) {
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${Math.round(bytes / 1e3)} KB`;
  return `${bytes} B`;
}

function renderDetails(b) {
  const frag = document.createDocumentFragment();
  const authors = b.authors.join(' & ');

  const cover = h('div', { class: 'details-cover' });
  fillCover(cover, { id: b.id, title: b.title, authors, hasCover: b.hasCover }, 'full');
  frag.append(cover);

  frag.append(h('h2', {}, b.title));
  const byline = h('p', { class: 'byline' });
  b.authors.forEach((a, i) => {
    if (i) byline.append(' & ');
    byline.append(h('button', { class: 'link', onclick: () => searchFor('author', a) }, a));
  });
  frag.append(byline);

  if (b.series) frag.append(h('p', { class: 'series-line' }, b.series));
  if (b.stars) {
    const full = Math.floor(b.stars);
    const half = b.stars - full >= 0.5;
    frag.append(h('div', { class: 'stars', title: `${b.stars} of 5` }, '★'.repeat(full) + (half ? '½' : '')));
  }

  const meta = h('dl', { class: 'meta' });
  const row = (label, value) => value && meta.append(h('dt', {}, label), h('dd', {}, value));
  row('Published', b.published);
  row('Publisher', b.publisher);
  row('Added', b.added);
  row('Language', b.languages.map(languageName).join(', '));
  for (const [k, v] of b.identifiers) row(k.toUpperCase(), v);
  frag.append(meta);

  if (b.tags.length) {
    frag.append(h('div', { class: 'chips' },
      ...b.tags.map((t) => h('button', { class: 'chip', onclick: () => searchFor('tag', t) }, t))));
  }

  if (b.formats.length) {
    frag.append(h('div', { class: 'section-label' }, 'Formats'));
    frag.append(h('div', { class: 'chips' }, ...b.formats.map((f) => h('span', {
      class: `chip static${f.present ? '' : ' missing'}`,
      title: f.present ? '' : 'File missing from the library folder',
    }, `${f.format} · ${formatSize(f.size)}`))));
  }

  if (b.commentsHtml) {
    frag.append(h('div', { class: 'section-label' }, 'Description'));
    const c = h('div', { class: 'comments' });
    c.innerHTML = b.commentsHtml; // sanitized in Rust (ammonia)
    frag.append(c);
  }

  frag.append(h('p', { class: 'folder' }, b.folder));
  return frag;
}

// ---------- Events ----------
els.grid.addEventListener('click', (e) => {
  const tile = e.target.closest('.tile');
  if (tile) select(Number(tile.dataset.id));
});

els.detailsClose.addEventListener('click', closeDetails);

// Never let links in book descriptions navigate the app window.
els.detailsBody.addEventListener('click', (e) => {
  if (e.target.closest('a')) e.preventDefault();
});

let searchTimer;
els.search.addEventListener('input', () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(refresh, 120);
});
els.search.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    if (els.search.value) { els.search.value = ''; refresh(); } else els.grid.focus();
    e.stopPropagation();
  } else if (e.key === 'Enter' || e.key === 'ArrowDown') {
    e.preventDefault();
    els.grid.focus();
    if (state.items.length && !state.indexById.has(state.selectedId)) select(state.items[0].id, { open: false, reveal: true });
  }
});

els.sort.addEventListener('change', () => {
  store.set('sort', els.sort.value);
  refresh();
});

els.libraryButton.addEventListener('click', chooseLibrary);
els.chooseLibrary.addEventListener('click', chooseLibrary);

document.addEventListener('keydown', (e) => {
  const inField = e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement;
  if ((e.key === '/' && !inField) || (e.key === 'f' && (e.metaKey || e.ctrlKey))) {
    e.preventDefault();
    els.search.focus();
    els.search.select();
    return;
  }
  if (inField) return;
  if (e.key === 'Escape') { closeDetails(); return; }

  const moves = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -state.cols, ArrowDown: state.cols };
  if (e.key in moves && state.items.length) {
    e.preventDefault();
    const cur = state.indexById.get(state.selectedId);
    const next = cur == null ? 0 : Math.min(state.items.length - 1, Math.max(0, cur + moves[e.key]));
    select(state.items[next].id, { open: !els.details.hidden, reveal: true });
  } else if ((e.key === 'Enter' || e.key === ' ') && state.selectedId != null) {
    e.preventDefault();
    showDetails(state.selectedId);
  }
});

// ---------- Start ----------
(async () => {
  els.sort.value = store.get('sort') || 'title';
  const last = store.get('libraryPath');
  if (!(last && await openLibrary(last))) await openLibrary(null);
})();
