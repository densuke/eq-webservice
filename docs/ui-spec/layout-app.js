// レイアウト設計システム案のページ: 目次・テーマ・比率の図・表を layout-system.js から組み立てる
(function () {
  "use strict";
  function h(tag, attrs, ...kids) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v == null || v === false) continue;
      if (k === "class") e.className = v; else if (k.startsWith("on")) e.addEventListener(k.slice(2), v); else e.setAttribute(k, v === true ? "" : v);
    }
    for (const k of kids.flat()) if (k != null && k !== false) e.append(k.nodeType ? k : document.createTextNode(String(k)));
    return e;
  }
  const $ = (s) => document.querySelector(s);
  const fill = (sel, kids) => { $(sel).replaceChildren(...kids); };

  // 目次・テーマ・ナビ
  fill("#toc", [...document.querySelectorAll("main section[id]")].map((s) => {
    const t = s.querySelector("h2"), num = t.querySelector(".num");
    const title = [...t.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent).join("").trim();
    return h("li", {}, h("a", { href: "#" + s.id }, num.textContent + ". " + title));
  }));
  $("#menu-btn").addEventListener("click", () => { const o = $("#nav").classList.toggle("open"); $("#menu-btn").setAttribute("aria-expanded", String(o)); });
  $("#nav").addEventListener("click", (e) => { if (e.target.closest("a")) $("#nav").classList.remove("open"); });
  const THEMES = ["auto", "light", "dark"], LABEL = { auto: "自動", light: "ライト", dark: "ダーク" };
  let theme = "auto";
  try { const t = localStorage.getItem("ui-spec-theme"); if (THEMES.includes(t)) theme = t; } catch (e) { /* 保存できなくても動く */ }
  const applyTheme = () => { theme === "auto" ? document.documentElement.removeAttribute("data-theme") : document.documentElement.setAttribute("data-theme", theme); $("#theme-btn").textContent = "表示: " + LABEL[theme]; };
  $("#theme-btn").addEventListener("click", () => { theme = THEMES[(THEMES.indexOf(theme) + 1) % 3]; try { localStorage.setItem("ui-spec-theme", theme); } catch (e) { /* 同上 */ } applyTheme(); });
  applyTheme();

  // 比率の図
  const COLORS = { bar: "#4f8cff", map: "#3ecf8e", overlay: "#f5a524", panel: "#b983ff", skip: "#8b949e" };
  const KIND = { bar: "帯・バー", map: "地図", overlay: "地図の上の重ね物", panel: "パネル", skip: "対象外" };
  let cur = window.LS_PROFILES[0].id;
  function draw() {
    const p = window.LS_PROFILES.find((q) => q.id === cur);
    $("#ls-ratio").textContent = p.ratio;
    $("#ls-basis").textContent = "根拠: " + p.basis + "。キャンバス " + p.canvas[0] + "×" + p.canvas[1];
    const canvas = $("#ls-canvas");
    const avail = Math.max(280, Math.min(900, canvas.parentElement.clientWidth - 24));
    const aspect = p.canvas[1] / p.canvas[0];
    // 縦長 (スマホ) は幅を絞り、横長は広く
    const w = aspect > 1.2 ? Math.min(avail, 360) : avail;
    canvas.style.width = w + "px";
    canvas.style.height = Math.round(w * aspect) + "px";
    canvas.replaceChildren(...p.boxes.map(([id, name, kind, x, y, bw, bh], i) => {
      const e = h("div", { class: "wf-box" + (kind === "map" ? " container" : ""), title: name + " — 幅 " + bw + "% / 高さ " + bh + "%", style: "--c:" + COLORS[kind] + ";left:" + x + "%;top:" + y + "%;width:" + bw + "%;height:" + bh + "%;z-index:" + (kind === "map" ? 1 : 2 + i) });
      const pxW = w * bw / 100, pxH = w * aspect * bh / 100;
      if (pxW >= 46 && pxH >= 14) { e.append(name); if (pxW >= 70 && pxH >= 28) e.append(h("span", { class: "sz" }, Math.round(bw) + "% × " + Math.round(bh) + "%")); }
      if (kind === "skip") e.style.borderStyle = "dashed";
      return e;
    }));
  }
  fill("#ls-controls", window.LS_PROFILES.map((p) => h("button", { type: "button", "aria-pressed": String(p.id === cur), "data-p": p.id, onclick: () => {
    cur = p.id; document.querySelectorAll("#ls-controls button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.p === cur))); draw();
  } }, p.label)));
  fill("#ls-legend", Object.entries(KIND).map(([k, v]) => h("span", {}, h("span", { class: "dot", style: "background:" + COLORS[k] }), v)));
  draw();
  let rt = 0; window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(draw, 120); });

  // 表
  function renderComponents() {
    const q = $("#lc-q").value.trim().toLowerCase();
    const rows = window.LS_COMPONENTS.filter((c) => !q || [c.name, c.merges, c.slot, c.note, c.id].join("\n").toLowerCase().includes(q));
    $("#lc-count").textContent = rows.length + " / " + window.LS_COMPONENTS.length + " 件";
    fill("#lc-table tbody", rows.map((c) => h("tr", {},
      h("td", {}, h("span", { class: "dot", style: "background:" + COLORS[c.role === "control" ? "bar" : c.role] }), h("b", {}, c.name)),
      h("td", { class: "small" }, c.merges), h("td", { class: "small" }, c.slot), h("td", { class: "small" }, c.size), h("td", { class: "small" }, c.priority), h("td", { class: "small muted" }, c.note))));
  }
  $("#lc-q").addEventListener("input", renderComponents);
  renderComponents();
  fill("#lt-table tbody", window.LS_TOKENS.map((t) => h("tr", {}, h("td", {}, h("b", {}, t[0])), h("td", { class: "small" }, t[1]), h("td", { class: "small muted" }, t[2]))));
  fill("#lq-list", window.LS_QUESTIONS.map((q) => h("li", {}, q)));
})();
