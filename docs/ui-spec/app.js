// 仕様書の表・図を data.js / measured.js から組み立てる。外部ライブラリなし。
(function () {
  "use strict";
  const C = window.COMPONENTS, R = window.REGIONS, M = window.MEASURED, N = window.NATIVE;

  /** 要素を作る。children は文字列 (テキスト) か Node。html は使わない */
  function h(tag, attrs, ...kids) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v == null || v === false) continue;
      if (k === "class") e.className = v;
      else if (k === "text") e.textContent = v;
      else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
      else e.setAttribute(k, v === true ? "" : v);
    }
    for (const k of kids.flat()) if (k != null && k !== false) e.append(k.nodeType ? k : document.createTextNode(String(k)));
    return e;
  }
  const $ = (s, r = document) => r.querySelector(s);
  const byId = Object.fromEntries(C.map((c) => [c.id, c]));
  const bySel = {};
  for (const c of C) if (!(c.sel in bySel)) bySel[c.sel] = c;
  const tag = (cls, text) => h("span", { class: "tag " + cls }, text);
  const confTag = (c) => tag({ 確: "sure", 中: "maybe", 要: "unknown" }[c] || "", c);
  const diffTag = (d) => tag({ 低: "lo", 中: "mid", 高: "hi" }[d] || "", d);
  const fill = (sel, kids) => { const e = $(sel); e.replaceChildren(...kids); return e; };

  // ---- 目次・テーマ・ナビ ----
  const sections = [...document.querySelectorAll("main section[id]")];
  fill("#toc", sections.map((s) => {
    const t = s.querySelector("h2");
    const num = t.querySelector(".num");
    const title = [...t.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent).join("").trim();
    return h("li", {}, h("a", { href: "#" + s.id, "data-for": s.id }, (num ? num.textContent + ". " : "") + title));
  }));
  const spy = new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting) {
      document.querySelectorAll(".nav a").forEach((a) => a.classList.toggle("active", a.dataset.for === e.target.id));
    }
  }, { rootMargin: "-10% 0px -80% 0px" });
  sections.forEach((s) => spy.observe(s));
  $("#menu-btn").addEventListener("click", () => {
    const open = $("#nav").classList.toggle("open");
    $("#menu-btn").setAttribute("aria-expanded", String(open));
  });
  $("#nav").addEventListener("click", (e) => { if (e.target.closest("a")) $("#nav").classList.remove("open"); });

  const THEMES = ["auto", "light", "dark"], THEME_LABEL = { auto: "自動", light: "ライト", dark: "ダーク" };
  let theme = "auto";
  try { theme = THEMES.includes(localStorage.getItem("ui-spec-theme")) ? localStorage.getItem("ui-spec-theme") : "auto"; } catch (e) { /* 保存できなくても動く */ }
  function applyTheme() {
    if (theme === "auto") document.documentElement.removeAttribute("data-theme"); else document.documentElement.setAttribute("data-theme", theme);
    $("#theme-btn").textContent = "表示: " + THEME_LABEL[theme];
  }
  $("#theme-btn").addEventListener("click", () => {
    theme = THEMES[(THEMES.indexOf(theme) + 1) % THEMES.length];
    try { localStorage.setItem("ui-spec-theme", theme); } catch (e) { /* 同上 */ }
    applyTheme();
    drawWireframe();
  });
  applyTheme();

  $("#cnt-components").textContent = String(C.filter((c) => c.kind !== "container" && !c.parent).length);

  // ---- 部品カタログ ----
  let regionFilter = "";
  let selectedId = "";
  const catBody = $("#cat-table tbody");
  function matches(c, q) {
    return !q || [c.id, c.name, c.sel, c.impl, c.show, c.place, c.couple, c.native, c.data].join("\n").toLowerCase().includes(q);
  }
  function renderCatalog() {
    const q = $("#cat-q").value.trim().toLowerCase();
    const rows = C.filter((c) => (!regionFilter || c.region === regionFilter) && matches(c, q));
    $("#cat-count").textContent = rows.length + " / " + C.length + " 件";
    catBody.replaceChildren(...rows.map((c) => h("tr", { class: "clickable" + (c.id === selectedId ? " sel" : ""), "data-id": c.id, tabindex: "0", role: "button", "aria-label": c.name + " の詳細" },
      h("td", {}, c.parent ? "└ " : "", h("b", {}, c.name)),
      h("td", {}, h("code", {}, c.sel)),
      h("td", {}, h("span", { class: "dot", style: "background:" + R[c.region].color }), R[c.region].name),
      h("td", { class: "small" }, c.show),
      h("td", { class: "c" }, diffTag(c.diff)),
      h("td", { class: "c" }, confTag(c.conf)))));
  }
  function showDetail(id, scroll) {
    const c = byId[id];
    if (!c) return;
    selectedId = id;
    renderCatalog();
    const m = measuredFor(c.sel);
    const card = $("#cat-detail");
    card.hidden = false;
    const row = (k, v) => v ? [h("dt", {}, k), h("dd", {}, v)] : [];
    card.replaceChildren(
      h("h3", {}, c.name, " ", confTag(c.conf), " ", diffTag(c.diff)),
      h("div", { class: "small muted" }, h("code", {}, c.id), "  ", h("code", {}, c.sel), "  ", R[c.region].name, c.parent ? "  (親: " + byId[c.parent].name + ")" : ""),
      h("dl", {},
        row("実装", c.impl), row("データ", c.data), row("表示条件", c.show), row("配置の決め方", c.place),
        row("他の部品との依存", c.couple), row("native (配信)", c.native), row("実測 (PC 1440×900 平時)", m), row("補足", c.note)),
    );
    if (scroll) card.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }
  function measuredFor(sel) {
    const parts = [];
    for (const [k, label] of [["pc1440/calm", "PC 平時"], ["pc1440/eew", "PC EEW 中"], ["phone/calm", "スマホ平時"]]) {
      const r = M[k] && M[k].rects[sel];
      if (r) parts.push(label + ": x" + r.x + " y" + r.y + " " + r.w + "×" + r.h);
    }
    return parts.length ? parts.join(" / ") : "";
  }
  catBody.addEventListener("click", (e) => { const tr = e.target.closest("tr[data-id]"); if (tr) showDetail(tr.dataset.id, true); });
  catBody.addEventListener("keydown", (e) => { if ((e.key === "Enter" || e.key === " ") && e.target.closest("tr[data-id]")) { e.preventDefault(); showDetail(e.target.closest("tr[data-id]").dataset.id, true); } });
  $("#cat-q").addEventListener("input", renderCatalog);
  fill("#cat-regions", [["", "すべて"], ...Object.entries(R).map(([k, v]) => [k, v.name])].map(([k, label]) =>
    h("button", { type: "button", class: "chip", "aria-pressed": String(k === ""), "data-r": k, onclick: (ev) => {
      regionFilter = k;
      document.querySelectorAll("#cat-regions button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.r === k)));
      renderCatalog();
    } }, label)));
  renderCatalog();

  // ---- 地図の層 ----
  fill("#layer-table tbody", window.LAYERS.map((l) => h("tr", {},
    h("td", { class: "c" }, String(l.order)), h("td", {}, h("code", {}, l.name)), h("td", {}, l.what), h("td", { class: "small" }, l.data),
    h("td", { class: "small" }, l.when), h("td", { class: "c" }, l.base ? "●" : "－"), h("td", { class: "small" }, l.note, " ", h("code", {}, l.impl)))));

  // ---- 表示条件 ----
  const S = window.STATES;
  $("#matrix-table thead").replaceChildren(h("tr", {}, h("th", {}, "部品"), ...S.map((s) => h("th", { class: "c", title: s.desc }, s.name)), h("th", {}, "補足")));
  fill("#matrix-table tbody", window.MATRIX.map(([id, cells, note]) => {
    const c = byId[id];
    return h("tr", {}, h("td", {}, h("b", {}, c ? c.name : id), " ", h("code", { class: "small" }, c ? c.sel : "")),
      ...[...cells].map((ch) => h("td", { class: "cell " + (ch === "●" ? "on" : ch === "○" ? "part" : "off") }, ch)),
      h("td", { class: "small muted" }, note));
  }));
  fill("#state-defs", S.flatMap((s) => [h("dt", { style: "font-weight:600" }, s.name), h("dd", { style: "margin:0" }, s.desc)]));

  // ---- 結合 ----
  fill("#coupling-list", window.COUPLINGS.map((c) => h("div", { class: "card cpl" },
    h("h4", {}, tag("hi", c.id), c.title),
    h("dl", {}, h("dt", {}, "ルール"), h("dd", {}, c.rule), h("dt", {}, "値の由来"), h("dd", {}, c.derived), h("dt", {}, "場所"), h("dd", {}, h("code", {}, c.where)),
      h("dt", {}, "壊れ方"), h("dd", {}, c.risk), h("dt", {}, "根拠"), h("dd", {}, c.ev)))));

  // ---- 表 (ブレークポイント・通信・native) ----
  fill("#bp-table tbody", window.BREAKPOINTS.map((b) => h("tr", {}, h("td", {}, h("code", {}, b.cond)), h("td", {}, b.target), h("td", { class: "small" }, b.effect), h("td", { class: "small" }, b.where))));
  fill("#flow-table tbody", window.DATAFLOW.map((d) => h("tr", {}, h("td", {}, tag("", d.kind)), h("td", {}, h("code", {}, d.path)), h("td", { class: "small" }, d.when), h("td", { class: "small" }, d.consumer), h("td", { class: "small" }, d.feeds))));
  fill("#native-table tbody", N.rows.map((r) => h("tr", {}, h("td", {}, h("b", {}, r[0])), h("td", { class: "small" }, r[1]), h("td", { class: "small" }, r[2]), h("td", { class: "small muted" }, r[3]))));

  // ---- 提案・障壁・確度 ----
  fill("#obstacles", window.OBSTACLES.map((o) => h("div", { class: "card cpl" },
    h("h4", {}, tag({ 高: "hi", 中: "mid", 低: "lo" }[o.sev], "影響 " + o.sev), o.n + ". " + o.title), h("p", { style: "margin:4px 0 0" }, o.body))));
  fill("#gaps", window.GAPS.map((g) => h("div", { class: "card cpl" }, h("h4", {}, g.t), h("p", { style: "margin:4px 0 0" }, g.d))));
  for (const [pre, J] of [["jdq", window.JDQ], ["jq", window.JQUAKE]]) {
    const rich = (t) => { const li = h("li", {}); li.innerHTML = t; return li; }; // data.js の固定文のみ (<b> を含む)
    fill("#" + pre + "-summary", J.summary.map(rich));
    fill("#" + pre + "-takeaways", J.takeaways.map(rich));
    fill("#" + pre + "-table tbody", J.rows.map((r) => h("tr", {}, h("td", {}, h("b", {}, r[0])), h("td", { class: "small" }, r[1]), h("td", { class: "small" }, r[2]), h("td", { class: "small" }, r[3]), h("td", { class: "c" }, r[4] === "—" ? "—" : confTag(r[4])))));
  }
  fill("#wanted", window.WANTED.map((w) => h("li", {}, h("b", {}, w.t), "  ", w.d)));
  $("#p-contract").textContent = window.PROPOSAL.contract;
  $("#p-profile").textContent = window.PROPOSAL.profile;
  fill("#p-steps", window.PROPOSAL.steps.map((s) => h("li", {}, h("b", {}, s.t), s.d)));
  fill("#p-questions", window.PROPOSAL.questions.map((q) => h("li", {}, q)));
  for (const [k, id] of [["sure", "#cf-sure"], ["maybe", "#cf-maybe"], ["unknown", "#cf-unknown"]]) fill(id, window.CONFIDENCE[k].map((t) => h("li", {}, t)));

  // ---- 実測値 ----
  const MLABEL = {
    "pc1440/calm": "PC 1440×900 平時", "pc1440/eew": "PC 1440×900 EEW 中 (デモ)", "hd1280/calm": "PC 1280×720 平時",
    "phone/calm": "スマホ縦 390 平時", "phone/eew": "スマホ縦 390 EEW 中 (デモ)", "phoneLand/calm": "横向きスマホ 844×390 平時", "tablet/calm": "タブレット縦 800×1000 平時",
  };
  fill("#m-sel", Object.keys(M).map((k) => h("option", { value: k }, MLABEL[k] || k)));
  function renderMeasured() {
    const k = $("#m-sel").value, d = M[k];
    $("#m-doc").textContent = "ページ全体 " + d.doc[0] + "×" + d.doc[1];
    const rows = Object.entries(d.rects).sort((a, b) => a[1].y - b[1].y || a[1].x - b[1].x);
    fill("#m-table tbody", rows.map(([sel, r]) => h("tr", {}, h("td", {}, h("code", {}, sel)), h("td", {}, bySel[sel] ? bySel[sel].name : ""),
      h("td", { class: "c" }, String(r.x)), h("td", { class: "c" }, String(r.y)), h("td", { class: "c" }, String(r.w)), h("td", { class: "c" }, String(r.h)), h("td", { class: "small" }, r.pos))));
  }
  $("#m-sel").addEventListener("change", renderMeasured);
  renderMeasured();

  // ---- ワイヤーフレーム ----
  const PROFILES = [
    { id: "pc-calm", label: "PC 1440×900 平時", key: "pc1440/calm" },
    { id: "pc-eew", label: "PC・EEW 中", key: "pc1440/eew" },
    { id: "phone-calm", label: "スマホ縦 390 平時", key: "phone/calm" },
    { id: "phone-eew", label: "スマホ縦・EEW 中", key: "phone/eew" },
    { id: "land", label: "横向きスマホ 844×390", key: "phoneLand/calm" },
    { id: "native", label: "native 配信 1280×720", key: null },
  ];
  let profile = PROFILES[0].id, wfSel = "";
  function profileBoxes(p) {
    if (p.key) {
      const d = M[p.key];
      const out = [];
      for (const [sel, r] of Object.entries(d.rects)) {
        const c = bySel[sel];
        if (!c || sel === "main.layout" || sel === ".offscreen-layer" || sel === ".legend .scale") continue;
        out.push({ id: c.id, name: c.name, region: c.region, container: c.kind === "container", x: r.x, y: r.y, w: r.w, h: r.h, sel });
      }
      return { w: d.doc[0], h: d.doc[1], boxes: out };
    }
    return { w: N.frame.w, h: N.frame.h, boxes: N.boxes.map(([id, name, region, x, y, w, hh, note]) => ({ id, name, region, container: ["topbar", "map", "side"].includes(id), x, y, w, h: hh, note })) };
  }
  function drawWireframe() {
    const p = PROFILES.find((q) => q.id === profile), d = profileBoxes(p);
    const stage = $("#wf-stage"), canvas = $("#wf-canvas");
    const avail = Math.max(260, stage.clientWidth - 24);
    const scale = Math.min(1, avail / d.w);
    canvas.style.width = Math.round(d.w * scale) + "px";
    canvas.style.height = Math.round(d.h * scale) + "px";
    const els = d.boxes.slice().sort((a, b) => (b.container - a.container) || (b.w * b.h - a.w * a.h)).map((b, i) => {
      const w = b.w * scale, hh = b.h * scale;
      const e = h("div", {
        class: "wf-box" + (b.container ? " container" : "") + (b.id === wfSel ? " sel" : ""), role: "button", tabindex: "0", title: b.name + " (" + b.w + "×" + b.h + ")",
        style: "--c:" + R[b.region].color + ";left:" + b.x * scale + "px;top:" + b.y * scale + "px;width:" + w + "px;height:" + hh + "px;z-index:" + (b.container ? 1 : 2 + i),
        onclick: () => selectBox(b, p),
        onkeydown: (ev) => { if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); selectBox(b, p); } },
      });
      if (w >= 44 && hh >= 14) { e.append(b.name); if (w >= 70 && hh >= 28) e.append(h("span", { class: "sz" }, b.w + "×" + b.h)); }
      return e;
    });
    canvas.replaceChildren(...els);
  }
  function selectBox(b, p) {
    wfSel = b.id;
    drawWireframe();
    const c = byId[b.id];
    const info = $("#wf-info");
    const head = [h("b", {}, b.name), " ", tag("", R[b.region].name), "  ", h("span", { class: "small muted" }, "x" + b.x + " y" + b.y + "  " + b.w + "×" + b.h + (b.sel ? "  " + b.sel : ""))];
    if (c) {
      info.replaceChildren(h("div", {}, head), h("p", { class: "small", style: "margin:6px 0 0" }, h("b", {}, "表示条件: "), c.show), h("p", { class: "small", style: "margin:4px 0 0" }, h("b", {}, "配置: "), c.place),
        h("p", { class: "small", style: "margin:4px 0 0" }, h("b", {}, "依存: "), c.couple), h("p", { class: "small", style: "margin:6px 0 0" }, h("a", { href: "#cat-table", onclick: () => showDetail(c.id, false) }, "カタログで詳細を見る")));
    } else info.replaceChildren(h("div", {}, head), b.note ? h("p", { class: "small muted", style: "margin:6px 0 0" }, b.note) : "");
  }
  fill("#wf-controls", PROFILES.map((p) => h("button", { type: "button", "aria-pressed": String(p.id === profile), "data-p": p.id, onclick: () => {
    profile = p.id; wfSel = "";
    document.querySelectorAll("#wf-controls button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.p === profile)));
    drawWireframe();
  } }, p.label)));
  fill("#wf-legend", Object.values(R).map((r) => h("span", {}, h("span", { class: "dot", style: "background:" + r.color }), r.name)));
  drawWireframe();
  let rt = 0;
  window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(drawWireframe, 120); });
})();
