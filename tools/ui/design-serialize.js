// Executa na página: converte o DOM renderizado em JSX do OpenPencil (camadas com posição absoluta).
window.__designSerialize = function (title) {
  const W = innerWidth, H = innerHeight;
  if (!document.getElementById('__pz_style')) {
    const s = document.createElement('style'); s.id = '__pz_style';
    s.textContent = '*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}[data-pz]::before,[data-pz]::after{content:none!important;display:none!important}';
    document.head.append(s);
  }
  document.querySelectorAll('.__pz').forEach(n => n.remove());
  document.querySelectorAll('[data-pz]').forEach(n => delete n.dataset.pz);
  const VOID = new Set(['INPUT', 'IMG', 'VIDEO', 'CANVAS', 'TEXTAREA', 'SELECT', 'svg']);
  // Materializa ::before/::after como spans reais para obter caixas medidas.
  const pending = [];
  for (const el of document.body.querySelectorAll('*')) {
    if (VOID.has(el.tagName) || el.closest('svg')) continue;
    for (const p of ['::before', '::after']) {
      const cs = getComputedStyle(el, p);
      if (!cs.content || cs.content === 'none' || cs.content === 'normal' || cs.display === 'none') continue;
      const span = document.createElement('span'); span.className = '__pz';
      for (const prop of cs) span.style.setProperty(prop, cs.getPropertyValue(prop));
      const m = cs.content.match(/^"(.*)"$/s); span.textContent = m ? m[1] : '';
      pending.push([el, p, span]);
    }
  }
  for (const [el, p, span] of pending) { el.dataset.pz = '1'; p === '::before' ? el.prepend(span) : el.append(span); }

  const n = v => Math.round(v * 10) / 10;
  const J = v => JSON.stringify(v);
  const ctx = document.createElement('canvas').getContext('2d');
  // Cor computada (rgb/rgba/color(srgb)) em #RRGGBB ou #RRGGBBAA.
  function hex(c) {
    let m = c.match(/^rgba?\(([^)]+)\)$/), v;
    if (m) v = m[1].split(/[\s,/]+/).filter(Boolean).map(parseFloat);
    else if ((m = c.match(/^color\(srgb ([^)]+)\)$/))) v = m[1].split(/[\s/]+/).filter(Boolean).map((x, i) => i < 3 ? parseFloat(x) * 255 : parseFloat(x));
    else { ctx.fillStyle = '#000'; ctx.fillStyle = c; return /^#/.test(ctx.fillStyle) ? ctx.fillStyle.toUpperCase() : (ctx.fillStyle === c ? '#000000' : hex(ctx.fillStyle)); }
    const h = x => Math.max(0, Math.min(255, Math.round(x))).toString(16).padStart(2, '0');
    return ('#' + h(v[0]) + h(v[1]) + h(v[2]) + (v.length > 3 && v[3] < 1 ? h(v[3] * 255) : '')).toUpperCase();
  }
  const clear = c => !c || c === 'transparent' || /^#[0-9A-F]{6}00$/.test(hex(c));
  const deferred = [];
  const name = el => {
    const cls = typeof el.className === 'string' ? el.className.trim().split(/\s+/).filter(c => c && c !== '__pz')[0] : '';
    const label = el.getAttribute?.('aria-label');
    return (cls ? cls : el.tagName.toLowerCase()) + (label ? ' · ' + label : '');
  };
  const attrs = o => Object.entries(o).filter(([, v]) => v !== undefined && v !== null && v !== '').map(([k, v]) => typeof v === 'string' && !v.startsWith('\u0000') ? `${k}=${J(v)}` : `${k}={${typeof v === 'string' ? v.slice(1) : J(v)}}`).join(' ');
  const raw = s => '\u0000' + s;
  const pos = (r, o) => ({ x: n(r.left - o.x), y: n(r.top - o.y), w: n(r.width), h: n(r.height) });
  function split(s) { const out = []; let d = 0, cur = ''; for (const ch of s) { if (ch === '(') d++; if (ch === ')') d--; if (ch === ',' && !d) { out.push(cur.trim()); cur = ''; } else cur += ch; } out.push(cur.trim()); return out; }
  function gradient(bi) {
    const m = bi.match(/^linear-gradient\((.*)\)$/s); if (!m) return null;
    const parts = split(m[1]);
    let dir = 'to bottom';
    if (!/^(rgb|#|color|transparent|hsl|oklch|lab)/.test(parts[0])) dir = parts.shift();
    let stops = parts.map((p, i) => { const mm = p.match(/^(.*?\))\s*([\d.]+%)?$/) || p.match(/^(\S+)\s*([\d.]+%)?$/); return [hex(mm[1]), mm[2] ? parseFloat(mm[2]) / 100 : i / Math.max(1, parts.length - 1)]; });
    const vertical = !/right|left|90deg|270deg/.test(dir);
    if (/to top|^0deg|to left|270deg/.test(dir)) stops = stops.map(([c, p]) => [c, n(1 - p)]).reverse();
    return raw(`[linearGradient(${J(stops)}${vertical ? ', { transform: [[0, 1, 0], [-1, 0, 1]] }' : ''})]`);
  }
  function visual(cs, r) {
    const p = {}; let has = false;
    if (!clear(cs.backgroundColor)) { p.bg = hex(cs.backgroundColor); has = true; }
    if (cs.backgroundImage.includes('linear-gradient') && !cs.backgroundImage.includes('url(') && /^(auto|auto auto|100% 100%|cover)$/.test(cs.backgroundSize)) {
      const g = gradient(split(cs.backgroundImage)[0]);
      if (g) { if (p.bg) p.fills = raw(`[solid(${J(p.bg)}), ...${g.slice(1)}]`); else p.fills = g; delete p.bg; has = true; }
    }
    const sides = ['top', 'right', 'bottom', 'left'].map(d => [d, parseFloat(cs.getPropertyValue(`border-${d}-width`)), cs.getPropertyValue(`border-${d}-style`), cs.getPropertyValue(`border-${d}-color`)]);
    const live = sides.filter(([, w, st, c]) => w > 0 && st !== 'none' && st !== 'hidden' && !clear(c));
    if (live.length) {
      p.stroke = hex(live[0][3]); has = true;
      if (live.length === 4 && live.every(x => x[1] === live[0][1])) { if (live[0][1] !== 1) p.strokeWidth = live[0][1]; }
      else p.strokeWeights = Object.fromEntries(sides.map(([d, w, st, c]) => [d, live.some(x => x[0] === d) ? w : 0]));
      if (live[0][2] === 'dashed') p.strokeDash = [3, 3]; else if (live[0][2] === 'dotted') p.strokeDash = [1, 2];
    }
    const rad = [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius].map(x => x.endsWith('%') ? Math.min(r.width, r.height) * parseFloat(x) / 100 : Math.min(parseFloat(x) || 0, Math.min(r.width, r.height) / 2)).map(n);
    if (rad.every(x => x === rad[0])) { if (rad[0]) p.rounded = rad[0]; }
    else [p.roundedTL, p.roundedTR, p.roundedBR, p.roundedBL] = rad.map(x => x || undefined);
    if (cs.boxShadow !== 'none') {
      for (const sh of split(cs.boxShadow)) {
        const col = sh.match(/(rgba?\([^)]+\)|color\([^)]+\)|#\w+)/)?.[0]; const nums = sh.replace(col, '').replace('inset', '').trim().split(/\s+/).map(parseFloat);
        if (!col || clear(col)) continue;
        if (sh.includes('inset')) { if (!p.stroke && nums[3] > 0 && !nums[0] && !nums[1] && !nums[2]) { p.stroke = hex(col); p.strokeWidth = nums[3]; has = true; } continue; }
        if (!p.shadow) { p.shadow = `${nums[0]} ${nums[1]} ${nums[2] || 0} ${hex(col)}`; has = true; }
      }
    }
    if (+cs.opacity < 1) { p.opacity = +(+cs.opacity).toFixed(2); has = true; }
    if (cs.overflowX !== 'visible' || cs.overflowY !== 'visible') { p.overflow = 'hidden'; has = true; }
    return [p, has];
  }
  function textProps(cs, single, h) {
    const fs = parseFloat(cs.fontSize);
    const p = { color: hex(cs.color), size: n(fs) };
    if (+cs.fontWeight !== 400) p.weight = +cs.fontWeight;
    if (/mono|consolas|cascadia|courier/i.test(cs.fontFamily)) p.font = 'Cascadia Mono';
    if (single) p.lineHeight = n(h);
    else { p.lineHeight = cs.lineHeight === 'normal' ? n(fs * 1.21) : n(parseFloat(cs.lineHeight)); const a = cs.textAlign === 'start' ? 'left' : cs.textAlign; if (a !== 'left') p.textAlign = a; }
    if (cs.letterSpacing !== 'normal' && parseFloat(cs.letterSpacing)) p.letterSpacing = n(parseFloat(cs.letterSpacing));
    p.textCase = { uppercase: 'upper', lowercase: 'lower', capitalize: 'title' }[cs.textTransform];
    if (/underline/.test(cs.textDecorationLine)) p.textDecoration = 'underline'; else if (/line-through/.test(cs.textDecorationLine)) p.textDecoration = 'strikethrough';
    p.textAutoResize = 'none';
    return p;
  }
  // ⋯ vem de fonte de fallback no Chrome; o OpenPencil desenha um retângulo vazio.
  const textJsx = (label, box, props, text) => `<Text ${attrs({ name: label, ...box, ...props })}>{${J(text.replace(/⋯/g, '···'))}}</Text>`;
  function textNode(node, o) {
    const pe = node.parentElement; const cs = getComputedStyle(pe);
    if (pe.checkVisibility && !pe.checkVisibility() && cs.display !== 'contents') return '';
    if (cs.visibility === 'hidden') return '';
    const pre = /^pre/.test(cs.whiteSpace);
    let text = pre ? node.textContent : node.textContent.replace(/\s+/g, ' ');
    if (!text.trim()) return '';
    const range = document.createRange(); range.selectNodeContents(node);
    const rects = [...range.getClientRects()].filter(x => x.width > 0);
    if (!rects.length) return '';
    const r = range.getBoundingClientRect();
    if (r.right < 0 || r.bottom < 0 || r.left > W || r.top > H) return '';
    const lines = new Set(rects.map(x => Math.round(x.top))).size;
    if (lines <= 1) text = text.trim();
    const box = pos({ left: r.left, top: r.top, width: Math.ceil(r.width) + (lines <= 1 ? 2 : 1), height: r.height }, o);
    return textJsx(text.slice(0, 40), box, textProps(cs, lines <= 1, r.height), text);
  }
  function svg(el, cs, r, o) {
    const c = el.cloneNode(true);
    const src = el.querySelectorAll('*'), dst = c.querySelectorAll('*');
    // Cada forma leva o traço já resolvido: o OpenPencil ignora atributos de apresentação no <svg>.
    src.forEach((s, i) => {
      if (!/^(path|circle|rect|line|polyline|polygon|ellipse)$/.test(s.tagName)) return;
      const k = getComputedStyle(s);
      for (const [a, v] of [['fill', k.fill], ['stroke', k.stroke], ['stroke-width', parseFloat(k.strokeWidth)], ['stroke-linecap', k.strokeLinecap], ['stroke-linejoin', k.strokeLinejoin]]) dst[i].setAttribute(a, /^(rgb|color)/.test(v) ? hex(v) : v);
    });
    const vb = el.getAttribute('viewBox') || `0 0 ${n(r.width)} ${n(r.height)}`;
    return `<svg x={${n(r.left - o.x)}} y={${n(r.top - o.y)}} width="${n(r.width)}" height="${n(r.height)}" viewBox="${vb}">${c.innerHTML.replace(/currentColor/g, hex(cs.color))}</svg>`;
  }
  const frame = (label, box, props, kids = '') => `<Frame ${attrs({ name: label, ...box, ...props })}${kids ? `>${kids}</Frame>` : ' />'}`;
  function control(el, cs, r, o) {
    const t = (el.type || '').toLowerCase(), box = pos(r, o);
    if (el.tagName === 'INPUT' && (t === 'checkbox' || t === 'radio') && cs.appearance !== 'none') {
      const col = hex(cs.accentColor !== 'auto' ? cs.accentColor : cs.color);
      return frame(t, box, { rounded: t === 'radio' ? n(r.width / 2) : 3, stroke: col, strokeWidth: 1.5, bg: el.checked ? col : undefined });
    }
    if (el.tagName === 'INPUT' && t === 'range') {
      const pct = (el.value - (el.min || 0)) / ((el.max || 100) - (el.min || 0)), x = n(pct * (r.width - 14)), col = hex(cs.color);
      return frame('volume', box, {}, frame('trilha', { x: 0, y: n(r.height / 2 - 2), w: n(r.width), h: 4 }, { bg: col, rounded: 2, opacity: 0.3 }) + frame('nível', { x: 0, y: n(r.height / 2 - 2), w: x + 7, h: 4 }, { bg: col, rounded: 2 }) + frame('alça', { x, y: n(r.height / 2 - 7), w: 14, h: 14 }, { bg: col, rounded: 7 }));
    }
    return null;
  }
  function field(el, cs, r) {
    const t = (el.type || '').toLowerCase();
    let text = '', color = cs.color, out = '';
    if (el.tagName === 'SELECT') text = el.selectedOptions[0]?.textContent ?? '';
    else if (el.value) text = t === 'password' ? '•'.repeat(el.value.length) : el.value;
    else if (el.placeholder) { text = el.placeholder; color = getComputedStyle(el, '::placeholder').color; }
    if (text) {
      const fs = parseFloat(cs.fontSize), lh = n(fs * 1.21), multi = el.tagName === 'TEXTAREA';
      const left = parseFloat(cs.paddingLeft) + parseFloat(cs.borderLeftWidth);
      const top = multi ? parseFloat(cs.paddingTop) + parseFloat(cs.borderTopWidth) : n((r.height - lh) / 2);
      const w = n(r.width - left - parseFloat(cs.paddingRight) - parseFloat(cs.borderRightWidth));
      const props = textProps(cs, !multi, lh); props.color = hex(color); delete props.textCase; delete props.textDecoration;
      out += textJsx(text.slice(0, 30), { x: n(left), y: top, w, h: multi ? n(r.height - top * 2) : lh }, props, text);
    }
    if (el.tagName === 'SELECT') out += `<svg x={${n(r.width - 26)}} y={${n(r.height / 2 - 8)}} width="16" height="16" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6" fill="none" stroke="${hex(cs.color)}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    const after = getComputedStyle(el, '::after');
    if (after.content && after.content !== 'none' && after.content !== 'normal' && after.position === 'absolute')
      out += frame('knob', { x: parseFloat(after.left), y: parseFloat(after.top), w: parseFloat(after.width), h: parseFloat(after.height) }, { bg: hex(after.backgroundColor), rounded: n(parseFloat(after.width) / 2) });
    return out;
  }
  function element(el, o, top) {
    if (el.nodeType === 3) return textNode(el, o);
    if (el.nodeType !== 1) return '';
    if (/^(SCRIPT|STYLE|TEMPLATE|NOSCRIPT|LINK|META|TITLE|BR|WBR|OPTION|DATALIST)$/.test(el.tagName)) return '';
    const cs = getComputedStyle(el);
    if (cs.display === 'none') return '';
    if (el.tagName !== 'svg' && el.checkVisibility && !el.checkVisibility()) return cs.display === 'contents' ? children(el, o) : '';
    if (!top && el.tagName === 'DIALOG' && el.matches(':modal')) { deferred.push([1e9, deferred.length, el]); return ''; }
    if (!top && el === document.fullscreenElement) return '';
    if (!top && cs.position !== 'static' && cs.zIndex !== 'auto' && +cs.zIndex > 0) { deferred.push([+cs.zIndex, deferred.length, el]); return ''; }
    if (cs.display === 'contents') return children(el, o);
    if (cs.visibility === 'hidden' && !el.querySelector('*')) return '';
    const r = el.getBoundingClientRect();
    const zero = r.width < 0.5 || r.height < 0.5;
    if (zero && (cs.overflowX !== 'visible' || el.tagName === 'svg')) return '';
    if (!zero && (r.right < 0 || r.bottom < 0 || r.left > W || r.top > H)) return '';
    if (el.tagName === 'svg') return svg(el, cs, r, o);
    const [vp, has] = visual(cs, r);
    if (zero || (cs.display === 'inline' && !has)) return children(el, o);
    const box = pos(r, o);
    if (el.tagName === 'VIDEO' || el.tagName === 'CANVAS') {
      const s = r.width / 1920;
      return frame('vídeo', box, { ...vp, bg: '#15302C', overflow: 'hidden' }, textJsx('conteúdo', { x: n(170 * s), y: n(450 * s), w: n(1500 * s), h: n(64 * s) }, { color: '#89E2C0', size: n(52 * s), lineHeight: n(64 * s), textAutoResize: 'none' }, 'Conteúdo fictício · demonstração do player'));
    }
    if (el.tagName === 'IMG') return frame('imagem', box, { ...vp, bg: '#2A2A2A' });
    // outline fica fora da caixa (seleção, atual, foco): vira um anel irmão pintado por cima.
    const ow = parseFloat(cs.outlineWidth), d = ow + (parseFloat(cs.outlineOffset) || 0);
    const ring = cs.outlineStyle !== 'none' && ow > 0 && !clear(cs.outlineColor)
      ? frame('contorno', { x: n(box.x - d), y: n(box.y - d), w: n(box.w + 2 * d), h: n(box.h + 2 * d) }, { stroke: hex(cs.outlineColor), strokeWidth: ow, rounded: vp.rounded ? n(vp.rounded + d) : undefined }) : '';
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return (control(el, cs, r, o) ?? frame(name(el), box, vp, field(el, cs, r))) + ring;
    return frame(name(el), box, vp, children(el, { x: r.left, y: r.top })) + ring;
  }
  // Ordem de pintura do CSS: irmãos em fluxo primeiro, posicionados depois.
  const positioned = c => c.nodeType === 1 && !/^(svg|SCRIPT|STYLE)$/.test(c.tagName) && getComputedStyle(c).position !== 'static';
  function children(el, o) {
    let flow = '', later = '';
    for (const c of el.childNodes) { const h = element(c, o, false); if (positioned(c)) later += h; else flow += h; }
    return flow + later;
  }

  const root = document.fullscreenElement || document.body;
  const bg = hex(getComputedStyle(document.body).backgroundColor);
  let jsx = element(root, { x: 0, y: 0 }, true);
  // Camadas com z-index e diálogos modais por cima, em ordem de pilha.
  const done = new Set();
  for (;;) {
    const next = deferred.filter(d => !done.has(d)).sort((a, b) => a[0] - b[0] || a[1] - b[1])[0];
    if (!next) break; done.add(next);
    const el = next[2];
    if (el.tagName === 'DIALOG') {
      const bd = getComputedStyle(el, '::backdrop');
      jsx += frame('fundo do diálogo', { x: 0, y: 0, w: W, h: H }, { bg: hex(bd.backgroundColor) });
    }
    jsx += element(el, { x: 0, y: 0 }, true);
  }
  for (const [el] of pending) delete el.dataset.pz;
  document.querySelectorAll('.__pz').forEach(n => n.remove());
  return { jsx: frame(title || 'tela', { w: W, h: H }, { bg, overflow: 'hidden' }, jsx), width: W, height: H };
};
