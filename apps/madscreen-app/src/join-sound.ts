import type { StreamEvent } from "@shared/client";

/** Um aviso por pessoa; hello/reconexão/prévia própria não representam entrada nova. */
export function createJoinNotice(selfId: string, notify: () => void) {
  const connections = new Map<string, string>();
  return (event: StreamEvent): void => {
    const known = event.uid && [...connections.values()].includes(event.uid);
    if (event.participants) {
      connections.clear();
      for (const person of event.participants) connections.set(person.viewerId, person.uid);
    }
    if (event.type === "viewer_joined" && event.viewerId && event.uid) {
      if (!connections.has(event.viewerId) && !known && event.uid !== selfId) notify();
      connections.set(event.viewerId, event.uid);
    }
    if (event.type === "viewer_left" && event.viewerId) connections.delete(event.viewerId);
    if (event.type === "ended" || event.viewers === 0) connections.clear();
  };
}

/** Timbre original curto, sintetizado localmente; não baixa arquivos nem captura áudio. */
export function createJoinSound() {
  let context: AudioContext | null = null;
  let disposed = false;
  let last = -Infinity;
  const unlock = () => {
    if (disposed) return;
    try {
      context ??= new AudioContext();
      if (context.state === "suspended") void context.resume().catch(() => undefined);
    } catch { /* O aviso visual de espectadores continua disponível. */ }
  };
  document.addEventListener("pointerdown", unlock, true);
  document.addEventListener("keydown", unlock, true);
  return {
    play() {
      if (disposed || performance.now() - last < 800) return;
      try { if (localStorage.getItem("ms.join-sound") === "off") return; } catch { /* preferência opcional */ }
      if (!context || context.state !== "running") return;
      last = performance.now();
      for (const [offset, frequency] of [[0, 660], [0.12, 880]] as const) {
        const oscillator = context.createOscillator();
        const gain = context.createGain();
        const start = context.currentTime + offset;
        oscillator.frequency.value = frequency;
        gain.gain.setValueAtTime(0, start);
        gain.gain.linearRampToValueAtTime(0.08, start + 0.015);
        gain.gain.exponentialRampToValueAtTime(0.001, start + 0.16);
        oscillator.connect(gain); gain.connect(context.destination);
        oscillator.onended = () => { oscillator.disconnect(); gain.disconnect(); };
        oscillator.start(start); oscillator.stop(start + 0.17);
      }
    },
    dispose() {
      disposed = true;
      document.removeEventListener("pointerdown", unlock, true);
      document.removeEventListener("keydown", unlock, true);
      if (context) void context.close().catch(() => undefined);
    },
  };
}
