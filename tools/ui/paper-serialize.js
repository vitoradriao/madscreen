// Executa na página: converte o DOM renderizado em HTML com posição absoluta e estilos inline para o Paper.
window.__paperSerialize = function (rootOverride) {
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
  const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const clear = c => !c || c === 'transparent' || /rgba\([^)]*,\s*0\)$/.test(c);
  const family = f => /mono|consolas|cascadia|courier/i.test(f) ? 'Cascadia Mono' : 'Inter';
  const deferred = [];
  const name = el => {
    const cls = typeof el.className === 'string' ? el.className.trim().split(/\s+/).filter(c => c && c !== '__pz')[0] : '';
    const label = el.getAttribute?.('aria-label');
    return (cls ? cls : el.tagName.toLowerCase()) + (label ? ' · ' + label : '');
  };
  function visual(cs) {
    const s = []; let has = false;
    if (!clear(cs.backgroundColor)) { s.push('background-color:' + cs.backgroundColor); has = true; }
    if (cs.backgroundImage.includes('gradient') && !cs.backgroundImage.includes('url(') && /^(auto|auto auto|100% 100%|cover)$/.test(cs.backgroundSize)) { s.push('background-image:' + cs.backgroundImage); has = true; }
    const sides = ['top', 'right', 'bottom', 'left'].map(d => [d, parseFloat(cs.getPropertyValue(`border-${d}-width`)), cs.getPropertyValue(`border-${d}-style`), cs.getPropertyValue(`border-${d}-color`)]);
    const live = sides.filter(([, w, st, c]) => w > 0 && st !== 'none' && st !== 'hidden' && !clear(c));
    if (live.length === 4 && live.every(x => x[1] === live[0][1] && x[2] === live[0][2] && x[3] === live[0][3])) { s.push(`border:${live[0][1]}px ${live[0][2]} ${live[0][3]}`); has = true; }
    else for (const [d, w, st, c] of live) { s.push(`border-${d}:${w}px ${st} ${c}`); has = true; }
    const r = [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius];
    if (r.some(x => parseFloat(x) > 0)) s.push('border-radius:' + r.join(' '));
    if (cs.boxShadow !== 'none') { s.push('box-shadow:' + cs.boxShadow); has = true; }
    if (+cs.opacity < 1) { s.push('opacity:' + cs.opacity); has = true; }
    if (cs.backdropFilter && cs.backdropFilter !== 'none') { s.push('backdrop-filter:' + cs.backdropFilter); has = true; }
    if (cs.overflowX !== 'visible' || cs.overflowY !== 'visible') { s.push('overflow:hidden'); has = true; }
    return [s, has];
  }
  function textStyle(cs, single, h) {
    const fs = parseFloat(cs.fontSize);
    const s = [`font-family:${family(cs.fontFamily)}`, `font-size:${cs.fontSize}`, `font-weight:${cs.fontWeight}`, `color:${cs.color}`];
    s.push(single ? `line-height:${n(h)}px;white-space:pre` : `line-height:${cs.lineHeight === 'normal' ? n(fs * 1.21) + 'px' : cs.lineHeight};text-align:${cs.textAlign === 'start' ? 'left' : cs.textAlign}`);
    if (cs.fontStyle === 'italic') s.push('font-style:italic');
    if (cs.letterSpacing !== 'normal' && parseFloat(cs.letterSpacing) !== 0) s.push('letter-spacing:' + cs.letterSpacing);
    if (cs.textTransform !== 'none') s.push('text-transform:' + cs.textTransform);
    if (/underline|line-through/.test(cs.textDecorationLine)) s.push('text-decoration:' + cs.textDecorationLine);
    if (cs.fontVariantNumeric !== 'normal') s.push('font-variant-numeric:' + cs.fontVariantNumeric);
    return s;
  }
  const box = (r, o, extra = '') => `position:absolute;left:${n(r.left - o.x)}px;top:${n(r.top - o.y)}px;width:${n(r.width)}px;height:${n(r.height)}px;${extra}`;
  function textDiv(text, cs, r, o, lines, label) {
    const single = lines <= 1;
    const style = box({ left: r.left, top: r.top, width: single ? Math.ceil(r.width) + 2 : Math.ceil(r.width) + 1, height: r.height }, o, textStyle(cs, single, r.height).join(';'));
    return `<div layer-name="${esc(label ?? text.slice(0, 40))}" style="${style}">${esc(text)}</div>`;
  }
  function textNode(node, o) {
    const pe = node.parentElement; const cs = getComputedStyle(pe);
    if (pe.checkVisibility && !pe.checkVisibility() && getComputedStyle(pe).display !== 'contents') return '';
    if (cs.visibility === 'hidden') return '';
    const pre = /^pre/.test(cs.whiteSpace);
    let text = pre ? node.textContent : node.textContent.replace(/\s+/g, ' ');
    if (!text.trim()) return '';
    const range = document.createRange(); range.selectNodeContents(node);
    const rects = [...range.getClientRects()].filter(x => x.width > 0);
    if (!rects.length) return '';
    const r = range.getBoundingClientRect();
    if (r.right < 0 || r.bottom < 0 || r.left > W || r.top > H) return '';
    const tops = new Set(rects.map(x => Math.round(x.top)));
    if (tops.size <= 1) text = text.trim();
    return textDiv(text, cs, r, o, tops.size);
  }
  function svg(el, cs, r, o) {
    const c = el.cloneNode(true);
    c.setAttribute('width', n(r.width)); c.setAttribute('height', n(r.height));
    for (const [a, p] of [['fill', 'fill'], ['stroke', 'stroke'], ['stroke-width', 'strokeWidth'], ['stroke-linecap', 'strokeLinecap'], ['stroke-linejoin', 'strokeLinejoin']]) if (!c.getAttribute(a)) c.setAttribute(a, cs[p]);
    c.removeAttribute('class'); c.setAttribute('style', box(r, o) + (+cs.opacity < 1 ? 'opacity:' + cs.opacity : ''));
    return c.outerHTML.replace(/currentColor/g, cs.color);
  }
  function control(el, cs, r, o, inner) {
    const t = (el.type || '').toLowerCase();
    if (el.tagName === 'INPUT' && (t === 'checkbox' || t === 'radio') && cs.appearance !== 'none') {
      const on = el.checked; const rad = t === 'radio' ? '50%' : '3px';
      return `<div layer-name="${t}" style="${box(r, o, `border-radius:${rad};border:1.5px solid ${on ? cs.accentColor !== 'auto' ? cs.accentColor : cs.color : cs.color};background-color:${on ? (cs.accentColor !== 'auto' ? cs.accentColor : cs.color) : 'transparent'}`)}"></div>`;
    }
    if (el.tagName === 'INPUT' && t === 'range') {
      const pct = (el.value - (el.min || 0)) / ((el.max || 100) - (el.min || 0));
      const x = n(pct * (r.width - 14));
      return `<div layer-name="range" style="${box(r, o)}"><div style="position:absolute;left:0;top:${n(r.height / 2 - 2)}px;width:${n(r.width)}px;height:4px;border-radius:2px;background-color:${cs.color};opacity:.3"></div><div style="position:absolute;left:0;top:${n(r.height / 2 - 2)}px;width:${x + 7}px;height:4px;border-radius:2px;background-color:${cs.color}"></div><div style="position:absolute;left:${x}px;top:${n(r.height / 2 - 7)}px;width:14px;height:14px;border-radius:50%;background-color:${cs.color}"></div></div>`;
    }
    let text = '', color = cs.color;
    if (el.tagName === 'SELECT') text = el.selectedOptions[0]?.textContent ?? '';
    else if (el.value) text = t === 'password' ? '•'.repeat(el.value.length) : el.value;
    else if (el.placeholder) { text = el.placeholder; color = getComputedStyle(el, '::placeholder').color; }
    let html = inner;
    if (text) {
      const fs = parseFloat(cs.fontSize), lh = n(fs * 1.21);
      const left = parseFloat(cs.paddingLeft), multi = el.tagName === 'TEXTAREA';
      const top = multi ? parseFloat(cs.paddingTop) : n((r.height - parseFloat(cs.borderTopWidth) - parseFloat(cs.borderBottomWidth) - lh) / 2);
      const w = n(r.width - left - parseFloat(cs.paddingRight) - parseFloat(cs.borderLeftWidth) * 2);
      html += `<div layer-name="${esc(text.slice(0, 30))}" style="position:absolute;left:${left}px;top:${top}px;width:${w}px;height:${multi ? n(r.height - top * 2) : lh}px;overflow:hidden;${textStyle({ ...cs, color, fontFamily: cs.fontFamily, fontSize: cs.fontSize, fontWeight: cs.fontWeight, fontStyle: cs.fontStyle, letterSpacing: cs.letterSpacing, textTransform: 'none', textDecorationLine: 'none', fontVariantNumeric: 'normal', lineHeight: cs.lineHeight, textAlign: cs.textAlign }, !multi, lh).join(';')}">${esc(text)}</div>`;
    }
    if (el.tagName === 'SELECT') html += `<svg style="position:absolute;left:${n(r.width - 26)}px;top:${n(r.height / 2 - 8)}px" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="${cs.color}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>`;
    return html;
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
    const [vs, has] = visual(cs);
    if (zero || (cs.display === 'inline' && !has)) return children(el, o);
    const label = name(el);
    if (el.tagName === 'VIDEO' || el.tagName === 'CANVAS') {
      const s = r.width / 1920;
      return `<div layer-name="vídeo" style="${box(r, o, 'background-color:#15302c;overflow:hidden;' + vs.join(';'))}"><div layer-name="conteúdo" style="position:absolute;left:${n(170 * s)}px;top:${n(450 * s)}px;width:${n(1500 * s)}px;height:${n(64 * s)}px;font-family:Inter;font-size:${n(52 * s)}px;line-height:${n(64 * s)}px;white-space:pre;color:#89e2c0">Conteúdo fictício · demonstração do player</div></div>`;
    }
    if (el.tagName === 'IMG') return `<div layer-name="imagem" style="${box(r, o, 'background-color:#2a2a2a;' + vs.join(';'))}"></div>`;
    const inner = { x: r.left + parseFloat(cs.borderLeftWidth), y: r.top + parseFloat(cs.borderTopWidth) };
    let body = '';
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) {
      const special = control(el, cs, r, o, '');
      if (special.startsWith('<div layer-name="checkbox"') || special.startsWith('<div layer-name="radio"') || special.startsWith('<div layer-name="range"')) return special;
      body = special;
      // ::after de input com appearance:none (interruptores).
      const after = getComputedStyle(el, '::after');
      if (after.content && after.content !== 'none' && after.content !== 'normal' && after.position === 'absolute')
        body += `<div layer-name="knob" style="position:absolute;left:${after.left};top:${after.top};width:${after.width};height:${after.height};border-radius:${after.borderTopLeftRadius};background-color:${after.backgroundColor}"></div>`;
    } else body = children(el, inner);
    return `<div layer-name="${esc(label)}" style="${box(r, o, vs.join(';'))}">${body}</div>`;
  }
  // Ordem de pintura do CSS: irmãos em fluxo primeiro, posicionados depois.
  const positioned = c => c.nodeType === 1 && !/^(svg|SCRIPT|STYLE)$/.test(c.tagName) && getComputedStyle(c).position !== 'static';
  function children(el, o) {
    let flow = '', pos = '';
    for (const c of el.childNodes) { const h = element(c, o, false); if (positioned(c)) pos += h; else flow += h; }
    return flow + pos;
  }

  const root = rootOverride || document.fullscreenElement || document.body;
  const bg = getComputedStyle(document.body).backgroundColor;
  let html = element(root, { x: 0, y: 0 }, true);
  // Camadas com z-index e diálogos modais por cima, em ordem de pilha.
  const done = new Set();
  while (true) {
    const next = deferred.filter(d => !done.has(d)).sort((a, b) => a[0] - b[0] || a[1] - b[1])[0];
    if (!next) break; done.add(next);
    const el = next[2];
    if (el.tagName === 'DIALOG') {
      const bd = getComputedStyle(el, '::backdrop');
      html += `<div layer-name="fundo do diálogo" style="position:absolute;left:0;top:0;width:${W}px;height:${H}px;background-color:${bd.backgroundColor}${bd.backdropFilter && bd.backdropFilter !== 'none' ? ';backdrop-filter:' + bd.backdropFilter : ''}"></div>`;
    }
    html += element(el, { x: 0, y: 0 }, true);
  }
  for (const [el] of pending) delete el.dataset.pz;
  document.querySelectorAll('.__pz').forEach(n => n.remove());
  return { html: `<div layer-name="tela" style="position:absolute;left:0;top:0;width:${W}px;height:${H}px;overflow:hidden;background-color:${bg}">${html}</div>`, bg, width: W, height: H };
};
