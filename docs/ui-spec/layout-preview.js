// レイアウトの定義を DOM の flex で描く。描いた結果の座標は window.__layoutRects (検算用)
(function () {
  "use strict";
  const C = window.LD_COMPONENTS, L = window.LD_LAYOUTS;
  const COLOR = { map: "#3ecf8e", panel: "#b983ff", bar: "#4f8cff", overlay: "#f5a524", skip: "#8b949e" };
  const $ = (s) => document.querySelector(s);
  const q = new URLSearchParams(location.search);
  let cur = L.find((l) => l.name === q.get("layout")) || L[0];
  let def = structuredClone(cur);
  let W = Number(q.get("w")) || cur.preview[0], H = Number(q.get("h")) || cur.preview[1];

  const sizeOf = (slot) => ({ ...(C[slot]?.size || {}), ...(def.sizes?.[slot] || {}) });

  /** 親の向き dir に沿った大きさを flex で決める */
  function applySize(el, size, dir, slot, page) {
    const m = /^fill(?::(\d+(?:\.\d+)?))?$/.exec(size || "fill");
    if (m && !(page && dir === "column")) { el.style.flex = `${m[1] || 1} 1 0`; return; }
    if (/^\d+(\.\d+)?(%|px)$/.test(size)) { el.style.flex = `0 0 ${size}`; return; }
    const vh = /^(\d+(?:\.\d+)?)vh$/.exec(size || "");
    if (vh) { el.style.flex = `0 0 ${(H * vh[1]) / 100}px`; return; }
    // auto (またはページスクロールの中の fill): 部品の既定の大きさ
    el.style.flex = "0 0 auto";
    const s = sizeOf(slot);
    const v = dir === "column" ? s.h : s.w;
    if (v === 0) el.hidden = true; // 0 は「このプロファイルでは出さない」
    else if (v != null) el.style[dir === "column" ? "height" : "width"] = v + "px";
  }

  function leaf(slot) {
    const c = C[slot];
    if (!c) throw new Error(`部品「${slot}」は LD_COMPONENTS に無い`);
    const el = document.createElement("div");
    el.className = "lp-box lp-leaf";
    // 名前は右下に (左上は重ね物で隠れやすい)
    el.style.alignItems = "flex-end";
    el.style.justifyContent = "flex-end";
    el.style.textAlign = "right";
    el.dataset.slot = slot;
    el.style.setProperty("--c", COLOR[c.kind]);
    el.innerHTML = `<div><b></b><span></span></div>`;
    el.querySelector("b").textContent = `${slot} — ${c.label}`;
    return el;
  }

  function overlayItem(item) {
    if (typeof item === "string") {
      const c = C[item];
      if (!c) throw new Error(`部品「${item}」は LD_COMPONENTS に無い`);
      const s = sizeOf(item);
      const el = document.createElement("div");
      el.className = "lp-ov";
      el.dataset.slot = item;
      el.style.setProperty("--c", COLOR[c.kind]);
      el.style.width = (s.w ?? 200) + "px";
      el.style.height = (s.h ?? 40) + "px";
      if (s.w === 0 || s.h === 0) el.hidden = true;
      el.innerHTML = "<b></b><span></span>";
      el.querySelector("b").textContent = item;
      return el;
    }
    const box = document.createElement("div");
    box.className = "lp-stack";
    box.style.flexDirection = item.flow || "column";
    box.style.alignItems = "inherit";
    for (const x of item.items || []) box.append(overlayItem(x));
    return box;
  }

  const CORNER = {
    "top-left": { left: 0, top: 0, align: "flex-start" },
    "top-right": { right: 0, top: 0, align: "flex-end" },
    "bottom-left": { left: 0, bottom: 0, align: "flex-start" },
    "bottom-right": { right: 0, bottom: 0, align: "flex-end" },
    top: { left: "50%", top: 0, align: "center", center: true },
    bottom: { left: "50%", bottom: 0, align: "center", center: true },
  };

  function overlays(el, ov) {
    if (!ov) return;
    const layer = document.createElement("div");
    layer.className = "lp-layer";
    for (const [corner, spec] of Object.entries(ov)) {
      const p = CORNER[corner];
      if (!p) throw new Error(`隅「${corner}」は使えない (top-left / top-right / bottom-left / bottom-right / top / bottom)`);
      const box = document.createElement("div");
      box.className = "lp-corner";
      for (const k of ["left", "right", "top", "bottom"]) if (p[k] != null) box.style[k] = typeof p[k] === "number" ? p[k] + "px" : p[k];
      if (p.center) box.style.transform = "translateX(-50%)";
      box.style.flexDirection = spec.flow || "column";
      box.style.alignItems = p.align;
      for (const x of spec.items || []) box.append(overlayItem(x));
      layer.append(box);
    }
    el.append(layer);
  }

  function node(n, parentDir, page) {
    let el;
    if (n.slot) el = leaf(n.slot);
    else {
      el = document.createElement("div");
      el.className = "lp-box";
      el.style.flexDirection = n.dir || "column";
      if (!n.children?.length) throw new Error("容器に children が無い");
      for (const ch of n.children) el.append(node(ch, n.dir || "column", page));
    }
    applySize(el, n.size, parentDir, n.slot, page);
    overlays(el, n.overlays);
    return el;
  }

  function render() {
    $("#lp-error").textContent = "";
    const frame = $("#lp-frame");
    const page = def.scroll === "page";
    let root;
    try {
      root = node(def.root, "column", page);
    } catch (e) {
      $("#lp-error").textContent = "定義の誤り: " + e.message;
      return;
    }
    root.style.flex = "1 1 auto";
    frame.replaceChildren(root);
    frame.style.width = W + "px";
    frame.style.height = page ? "auto" : H + "px";
    frame.style.minHeight = H + "px";
    frame.style.display = "flex";
    frame.style.flexDirection = "column";
    const stage = $("#lp-stage");
    const k = Math.min(1, (stage.clientWidth - 20) / W);
    frame.style.transform = `scale(${k})`;
    stage.style.height = Math.ceil(frame.offsetHeight * k) + 20 + "px";
    // 描いた結果の大きさを書き込む
    const fr = frame.getBoundingClientRect();
    const rects = {};
    for (const el of frame.querySelectorAll("[data-slot]")) {
      if (el.hidden) continue;
      const r = el.getBoundingClientRect();
      const x = Math.round((r.left - fr.left) / k), y = Math.round((r.top - fr.top) / k), w = Math.round(r.width / k), h = Math.round(r.height / k);
      rects[el.dataset.slot] = { x, y, w, h };
      el.querySelector("span").textContent = `${w}×${h} (${Math.round((w / W) * 100)}% × ${Math.round((h / H) * 100)}%)`;
    }
    window.__layoutRects = rects;
    // 検査: 重ね物どうしの重なり (隅ごとに積むだけでは、別の隅の部品と重なりうる)
    const ovs = [...frame.querySelectorAll(".lp-ov[data-slot]")].filter((e) => !e.hidden).map((e) => e.dataset.slot);
    const hit = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    const warns = [];
    for (let i = 0; i < ovs.length; i++) for (let j = i + 1; j < ovs.length; j++) if (hit(rects[ovs[i]], rects[ovs[j]])) warns.push(`${ovs[i]} と ${ovs[j]}`);
    for (const e of frame.querySelectorAll(".lp-ov")) e.style.outline = "";
    for (const w of warns) for (const n of w.split(" と ")) frame.querySelector(`.lp-ov[data-slot="${n}"]`).style.outline = "3px solid #ff4d4f";
    window.__layoutWarnings = warns;
    if (warns.length) $("#lp-error").textContent = "警告: 重ね物が重なっている — " + warns.join(" / ");
    $("#lp-note").textContent = `${def.name}: ${def.note || ""}  — 条件 when: ${JSON.stringify(def.when || {})}  /  描いた画面 ${W}×${H}${page ? " (ページ全体がスクロール)" : ""}`;
  }

  function pick() {
    const ok = (w) => (w.minWidth == null || W >= w.minWidth) && (w.maxWidth == null || W <= w.maxWidth) && (w.minHeight == null || H >= w.minHeight) && (w.maxHeight == null || H <= w.maxHeight);
    // 配信用 (target: broadcast) は画面の大きさでは選ばない。上から順に最初に合うもの
    return L.find((l) => l.when?.target !== "broadcast" && ok(l.when || {})) || L[0];
  }

  function select(l, keepSize) {
    cur = l;
    def = structuredClone(l);
    if (!keepSize) [W, H] = l.preview;
    $("#lp-layout").value = l.name;
    $("#lp-w").value = W;
    $("#lp-h").value = H;
    $("#lp-code").value = JSON.stringify(def, null, 2);
    render();
  }

  $("#lp-layout").replaceChildren(...L.map((l) => Object.assign(document.createElement("option"), { value: l.name, textContent: l.name })));
  $("#lp-layout").addEventListener("change", (e) => select(L.find((l) => l.name === e.target.value)));
  $("#lp-pick").addEventListener("click", () => { W = Number($("#lp-w").value); H = Number($("#lp-h").value); select(pick(), true); });
  for (const id of ["#lp-w", "#lp-h"]) $(id).addEventListener("change", () => { W = Number($("#lp-w").value); H = Number($("#lp-h").value); render(); });
  $("#lp-apply").addEventListener("click", () => {
    try { def = JSON.parse($("#lp-code").value); render(); } catch (e) { $("#lp-error").textContent = "JSON の誤り: " + e.message; }
  });
  $("#lp-reset").addEventListener("click", () => select(cur, true));
  let rt = 0;
  window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(render, 120); });
  select(cur, q.has("w"));
})();
