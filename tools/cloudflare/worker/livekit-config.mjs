/** Lê somente os três campos LiveKit; mensagens de erro nunca refletem os valores. */
export function parseLiveKitConfig(raw) {
  const config = {};
  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim() || line.trimStart().startsWith("#")) continue;
    const index = line.indexOf("=");
    const key = line.slice(0, index).trim();
    if (index < 0 || !["LIVEKIT_URL", "LIVEKIT_API_KEY", "LIVEKIT_API_SECRET"].includes(key)) continue;
    if (Object.hasOwn(config, key)) throw new Error(`Campo ${key} duplicado no arquivo privado.`);
    let value = line.slice(index + 1).trim();
    if (value.startsWith('"')) {
      try { value = JSON.parse(value); } catch { throw new Error(`Campo ${key} inválido.`); }
    } else if (value.startsWith("'") && value.endsWith("'")) value = value.slice(1,-1);
    if (typeof value !== "string" || !value || /[\r\n]/.test(value)) throw new Error(`Campo ${key} inválido.`);
    config[key] = value;
  }
  for (const key of ["LIVEKIT_URL", "LIVEKIT_API_KEY", "LIVEKIT_API_SECRET"]) {
    if (!config[key]) throw new Error(`Arquivo privado sem ${key} válido.`);
  }
  let url;
  try { url = new URL(config.LIVEKIT_URL); } catch { throw new Error("LIVEKIT_URL inválida."); }
  if (url.protocol !== "wss:" || url.username || url.password || url.search || url.hash || url.pathname !== "/") throw new Error("LIVEKIT_URL deve ser uma origem wss:// válida.");
  return config;
}
