/** A tiny HTML template: interpolated values are escaped unless they are `Html` already. */

export class Html {
  constructor(readonly value: string) {}
  toString(): string {
    return this.value;
  }
}

type Part = Html | string | number | boolean | null | undefined | readonly Part[];

export function escape(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

function render(part: Part): string {
  if (part === null || part === undefined || part === false || part === true) return "";
  if (part instanceof Html) return part.value;
  if (Array.isArray(part)) return part.map(render).join("");
  return escape(String(part));
}

export function html(strings: TemplateStringsArray, ...parts: Part[]): Html {
  let out = strings[0]!;
  for (let i = 0; i < parts.length; i++) out += render(parts[i]!) + strings[i + 1]!;
  return new Html(out);
}

/** Trusted markup, such as inline SVG written here. */
export function raw(s: string): Html {
  return new Html(s);
}
