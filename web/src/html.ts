// HTML に文字列を埋め込むときのエスケープ (本文・属性の値のどちらにも使える)。

export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}
