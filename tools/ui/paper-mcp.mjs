// Cliente mínimo do MCP local do Paper Desktop (HTTP + SSE), usado por capture-redesign.mjs.
const endpoint = process.env.MADSCREEN_PAPER_MCP || 'http://127.0.0.1:29979/mcp';
let sid, seq = 0;
async function post(body) {
  const res = await fetch(endpoint, { method: 'POST', headers: { 'Content-Type': 'application/json', Accept: 'application/json, text/event-stream', ...(sid ? { 'mcp-session-id': sid } : {}) }, body: JSON.stringify(body) });
  sid ??= res.headers.get('mcp-session-id');
  return res.text();
}
async function rpc(method, params) {
  const id = ++seq;
  const text = await post({ jsonrpc: '2.0', id, method, params });
  const msgs = text.split('\n').filter(l => l.startsWith('data:')).map(l => JSON.parse(l.slice(5)));
  const msg = msgs.find(m => m.id === id) ?? (text.trim().startsWith('{') ? JSON.parse(text) : null);
  if (!msg) throw Error('Resposta vazia do Paper: ' + text.slice(0, 300));
  if (msg.error) throw Error(JSON.stringify(msg.error));
  return msg.result;
}
export async function connect() {
  await rpc('initialize', { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'madscreen-capture', version: '1' } });
  await post({ jsonrpc: '2.0', method: 'notifications/initialized' });
}
// O último bloco de conteúdo é o resultado; o primeiro é o cabeçalho do arquivo.
export async function tool(name, args) {
  const r = await rpc('tools/call', { name, arguments: args });
  const parts = (r.content ?? []).map(c => c.text ?? '');
  if (r.isError) throw Error(name + ': ' + parts.join('\n').slice(0, 800));
  try { return JSON.parse(parts.at(-1)); } catch { return parts.at(-1); }
}
