import type { WatchSession } from "./client";

type Report = ReturnType<WatchSession["getDiagnostics"]>;
const unknown = "Não foi possível determinar. Copie o diagnóstico logo após o problema.";

/** Observações do intervalo, nunca diagnóstico causal. Avaliado só ao abrir a ajuda. */
export function diagnosticGuidance(report: Report | null, now: number, hidden: boolean): string[] {
  const host = "Transmissão: dados do computador do amigo não estão disponíveis aqui. Peça para conferir a fonte e o som e copiar o diagnóstico da transmissão.";
  const unavailable = [host, `Recepção: ${unknown}`, `Decodificação: ${unknown}`, `Apresentação: ${unknown}`];
  const samples = report?.samples;
  if (hidden || !samples || samples.length < 2) return unavailable;
  const a = samples.at(-2)!, b = samples.at(-1)!;
  const gap = b.atMs - a.atMs;
  if (a.segment !== b.segment || a.provider !== b.provider || a.documentHidden || b.documentHidden
    || !Number.isFinite(now) || now < b.atMs || now - b.atMs > 2500 || gap < 100 || gap > 2500
    || [a.atMs, b.atMs, a.bytesReceived, b.bytesReceived, a.framesDecoded, b.framesDecoded, a.packetsLost, b.packetsLost].some(value => !Number.isFinite(value))
    || b.bytesReceived < a.bytesReceived || b.framesDecoded < a.framesDecoded || b.packetsLost < a.packetsLost) return unavailable;
  const received = b.bytesReceived > a.bytesReceived;
  const decoded = b.framesDecoded > a.framesDecoded;
  const presented = a.presentedFrames !== null && b.presentedFrames !== null
    && Number.isFinite(a.presentedFrames) && Number.isFinite(b.presentedFrames) && b.presentedFrames >= a.presentedFrames;
  if ((!received && decoded) || (presented && b.presentedFrames! > a.presentedFrames! && !decoded)) return unavailable;
  return [host,
    `Recepção: ${received ? "chegaram dados de vídeo no último intervalo." : "não houve novos dados de vídeo neste intervalo; confira se a fonte continua ativa e aguarde a reconexão."}${b.packetsLost > a.packetsLost ? " O contador de perdas aumentou; isso não identifica a causa da pausa." : ""} RTT é o tempo de ida e volta da conexão, não o atraso da imagem.`,
    `Decodificação: ${decoded ? "novos quadros foram decodificados." : "nenhum novo quadro foi decodificado neste intervalo. Fonte parada também pode produzir esse resultado; copie o diagnóstico se a imagem deveria estar mudando."}`,
    `Apresentação: ${!presented ? unknown : b.presentedFrames! > a.presentedFrames! ? "novos quadros foram apresentados. Isso não comprova fluidez ou sincronia com o áudio." : "nenhum novo quadro foi apresentado neste intervalo. Confira se a fonte está em movimento e copie o diagnóstico se a pausa persistir."}`];
}
