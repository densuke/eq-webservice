// サーバの WebSocket の URL。connection.ts から分けて、node のテストで読めるようにしている。

/** ページの場所からの相対パスで作る。配信用の表示 (?broadcast) は ws にも付けて名乗り、web の同接に数えさせない */
export function wsUrl(href: string): string {
  const page = new URL(href);
  const u = new URL("ws", page);
  u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
  if (page.searchParams.has("broadcast")) u.search = "broadcast=1";
  return u.toString();
}
