export { banner, el, segmented, svg } from "@shared/dom";

export interface View {
  root: HTMLElement;
  /** Chamado quando a aba é aberta. */
  show?(): void;
  /** Chamado quando outra aba assume. */
  hide?(): void;
  /** Libera recursos ao encerrar a sessão da interface. */
  dispose?(): void;
}
