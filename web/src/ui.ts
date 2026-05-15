export type Tone = "good" | "warn" | "bad";

export function sectionShell(id: string, title: string, child?: HTMLElement): HTMLElement {
  const section = element("section", "section");
  section.id = id;
  section.append(textElement("h2", "section-title", title));
  if (child) {
    section.append(child);
  }

  return section;
}

export function statusPill(text: string, tone: Tone): HTMLElement {
  return textElement("span", `status-pill ${tone}`, text);
}

export function icon(className: string): HTMLSpanElement {
  const span = element("span", `icon ${className}`);
  span.setAttribute("aria-hidden", "true");

  return span;
}

export function textElement<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string | undefined,
  text: string
): HTMLElementTagNameMap[K] {
  const node = element(tag, className);
  node.textContent = text;

  return node;
}

export function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }

  return node;
}
