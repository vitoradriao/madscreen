import type { FriendView } from "@shared/client";
import { icon } from "@shared/icons";
import { createWatchLayout, type WatchLayout } from "@shared/ui";
import { el, svg, type View } from "../dom";
import { client } from "../session";
import { toggleFullscreen } from "./fullscreen";
import { openMiniPlayer } from "../mini-player";

export interface WatchView extends View {
  watch(friend: Pick<FriendView, "username" | "displayName">): void;
  panel: WatchLayout;
  compact: HTMLElement;
}

/** Até duas transmissões pelo provedor selecionado no Worker, também em segundo plano. */
export function createWatchView(onExpand: () => void, onAdd?: () => void): WatchView {
  const expand = el("button", { class: "btn ghost sm", type: "button", textContent: "Voltar para Assistir" });
  expand.addEventListener("click", () => {
    onExpand();
    panel.root.focus({ preventScroll: true });
  });
  const compact = el("aside", { class: "watch-compact", hidden: true }, expand);
  compact.setAttribute("aria-label", "Transmissão em reprodução");
  const panel = createWatchLayout(client, {
    ...(onAdd ? { onAdd } : {}),
    toggleFullscreen,
    popout: openMiniPlayer,
    onLeave: () => {
      started = false;
      compact.hidden = true;
      root.replaceChildren(empty);
    },
  });
  panel.root.tabIndex = -1;
  const empty = el(
    "div",
    { class: "empty-main fade-in" },
    svg(icon("monitor")),
    el("h2", {}, "Escolha uma transmissão ao vivo"),
    el("p", { class: "small", textContent: "Abra um canal em Servidores e use o botão Assistir de uma transmissão." }),
  );
  if (onAdd) {
    const browse = el("button", { class: "btn primary", type: "button", textContent: "Explorar canais" });
    browse.addEventListener("click", onAdd);
    empty.append(browse);
  }
  const root = el("section", { class: "view view-wide watch-view" }, empty);
  let started = false;
  let visible = false;
  return {
    root,
    panel,
    compact,
    dispose: () => {
      compact.hidden = true;
      panel.dispose();
    },
    watch(friend) {
      panel.watch(friend);
      started = true;
      (visible ? root : compact).append(panel.root);
      if (visible) empty.remove();
      compact.hidden = visible;
    },
    show: () => {
      visible = true;
      compact.hidden = true;
      if (started) root.replaceChildren(panel.root);
    },
    hide: () => {
      visible = false;
      if (started) {
        compact.append(panel.root);
        compact.hidden = false;
      }
    },
  };
}
