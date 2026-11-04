/** Cria elementos sem innerHTML com dados externos. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { class?: string } = {},
  ...children: (Node | string | null | undefined | false)[]
): HTMLElementTagNameMap[K] {
  const { class: className, ...rest } = props;
  const node = document.createElement(tag);
  if (className) node.className = className;
  Object.assign(node, rest);
  for (const child of children) if (child != null && child !== false) node.append(child);
  return node;
}

/** SVG estático de `icons.ts` (nunca de dados externos). */
export function svg(markup: string): HTMLElement {
  const holder = document.createElement("span");
  holder.style.display = "inline-flex";
  holder.innerHTML = markup;
  return holder;
}

export function banner(kind: "error" | "info" | "ok", text: string): HTMLElement {
  return el("div", { class: `banner ${kind}`, role: "status" }, text);
}

export function segmented<T extends number | string>(
  options: { value: T; label: string }[],
  initial: T,
  onChange: (value: T) => void,
): { root: HTMLElement; set(value: T): void; setDisabled(disabled: boolean): void } {
  const root = el("div", { class: "segmented", role: "radiogroup" });
  const items: { value: T; button: HTMLButtonElement }[] = [];
  let current = initial;

  const set = (value: T): void => {
    current = value;
    for (const item of items) {
      const selected = item.value === value;
      item.button.setAttribute("aria-checked", String(selected));
      item.button.tabIndex = selected ? 0 : -1;
    }
  };

  const enabled = (): { value: T; button: HTMLButtonElement }[] => items.filter((item) => !item.button.disabled);

  const move = (from: HTMLButtonElement, to: number | "first" | "last"): void => {
    const choices = enabled();
    if (choices.length === 0) return;
    const index = choices.findIndex((item) => item.button === from);
    const next = to === "first" ? 0
      : to === "last" ? choices.length - 1
      : ((index === -1 ? 0 : index) + to + choices.length) % choices.length;
    const item = choices[next];
    if (!item) return;
    if (item.value !== current) {
      set(item.value);
      onChange(item.value);
    }
    item.button.focus();
  };

  for (const option of options) {
    const button = el("button", { type: "button", textContent: option.label });
    button.setAttribute("role", "radio");
    button.addEventListener("click", () => {
      set(option.value);
      onChange(option.value);
    });
    items.push({ value: option.value, button });
    root.append(button);
  }

  root.addEventListener("keydown", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLButtonElement) || target.disabled || !items.some((item) => item.button === target)) return;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") move(target, 1);
    else if (event.key === "ArrowLeft" || event.key === "ArrowUp") move(target, -1);
    else if (event.key === "Home") move(target, "first");
    else if (event.key === "End") move(target, "last");
    else return;
    event.preventDefault();
  });

  set(initial);
  return {
    root,
    set,
    setDisabled(disabled) {
      for (const item of items) item.button.disabled = disabled;
      set(current);
    },
  };
}

export function initials(name: string): string {
  return name.trim().slice(0, 2) || "?";
}

export function ago(since: number): string {
  const minutes = Math.max(0, Math.floor((Date.now() / 1000 - since) / 60));
  if (minutes < 1) return "agora";
  if (minutes < 60) return `há ${minutes} min`;
  return `há ${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
