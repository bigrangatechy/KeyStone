/* SPDX-FileCopyrightText: 2026 The KeyStone Authors
 * SPDX-License-Identifier: GPL-2.0-or-later */

(function () {
  const tabs = document.querySelector("[data-tabs]");
  if (tabs) {
    tabs.querySelectorAll("[data-tab]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const id = btn.getAttribute("data-tab");
        tabs.querySelectorAll("[data-tab]").forEach((b) => b.classList.remove("active"));
        document.querySelectorAll("[data-panel]").forEach((p) => p.classList.remove("active"));
        btn.classList.add("active");
        const panel = document.querySelector('[data-panel="' + id + '"]');
        if (panel) panel.classList.add("active");
      });
    });
    const want = new URLSearchParams(location.search).get("panel");
    if (want) {
      const btn = tabs.querySelector('[data-tab="' + want + '"]');
      if (btn) btn.click();
    }
  }

  function parse(el) {
    try {
      return JSON.parse(el.getAttribute("data-json") || "null");
    } catch (e) {
      return null;
    }
  }

  function actionForm(node, op, payload) {
    const f = document.createElement("form");
    f.method = "post";
    f.action = "/nodes/" + encodeURIComponent(node) + "/docker/" + op;
    f.className = "inline";
    const i = document.createElement("input");
    i.type = "hidden";
    i.name = "payload";
    i.value = JSON.stringify(payload);
    f.appendChild(i);
    const b = document.createElement("button");
    b.type = "submit";
    b.textContent = op.replace("container_", "").replace("image_", "").replace("volume_", "").replace("network_", "").replace("compose_", "");
    f.appendChild(b);
    return f;
  }

  function renderContainers(el) {
    const data = parse(el);
    const node = el.getAttribute("data-node");
    if (!Array.isArray(data)) {
      el.textContent = el.getAttribute("data-json") || "n/a";
      return;
    }
    const table = document.createElement("table");
    table.innerHTML = "<thead><tr><th>Name</th><th>Image</th><th>State</th><th>Project</th><th></th></tr></thead>";
    const body = document.createElement("tbody");
    data.forEach((c) => {
      const tr = document.createElement("tr");
      const name = (c.names && c.names[0]) ? c.names[0].replace(/^\//, "") : c.id;
      const id = c.id_full || c.id;
      tr.innerHTML = "<td>" + name + "<br><code>" + (c.id || "") + "</code></td><td>" +
        (c.image || "") + "</td><td>" + (c.state || "") + " " + (c.status || "") +
        "</td><td>" + (c.compose_project || "") + "</td>";
      const td = document.createElement("td");
      ["container_start", "container_stop", "container_restart", "container_kill", "container_remove"].forEach((op) => {
        td.appendChild(actionForm(node, op, { id: id }));
      });
      const logs = document.createElement("a");
      logs.href = "/nodes/" + encodeURIComponent(node) + "/containers/" + encodeURIComponent(id) + "/logs";
      logs.textContent = "logs";
      td.appendChild(logs);
      const stats = document.createElement("a");
      stats.href = "/nodes/" + encodeURIComponent(node) + "/containers/" + encodeURIComponent(id) + "/stats";
      stats.textContent = " stats";
      td.appendChild(stats);
      tr.appendChild(td);
      body.appendChild(tr);
    });
    table.appendChild(body);
    el.replaceChildren(table);
  }

  function renderGeneric(el, rowsFn) {
    const data = parse(el);
    const node = el.getAttribute("data-node");
    el.replaceChildren(rowsFn(data, node) || document.createTextNode(JSON.stringify(data, null, 2)));
  }

  const containers = document.getElementById("containers");
  if (containers) renderContainers(containers);

  const compose = document.getElementById("compose");
  if (compose) {
    const data = parse(compose);
    const node = compose.getAttribute("data-node");
    const wrap = document.createElement("div");
    if (data && typeof data === "object") {
      Object.keys(data).forEach((project) => {
        const h = document.createElement("h3");
        h.textContent = project;
        wrap.appendChild(h);
        wrap.appendChild(actionForm(node, "compose_up", { project: project }));
        wrap.appendChild(actionForm(node, "compose_down", { project: project }));
        wrap.appendChild(actionForm(node, "compose_pull", { project: project }));
        const pre = document.createElement("pre");
        pre.textContent = JSON.stringify(data[project], null, 2);
        wrap.appendChild(pre);
      });
    } else {
      wrap.textContent = compose.getAttribute("data-json") || "";
    }
    compose.replaceChildren(wrap);
  }

  const images = document.getElementById("images");
  if (images) {
    const data = parse(images);
    const node = images.getAttribute("data-node");
    if (Array.isArray(data)) {
      const table = document.createElement("table");
      table.innerHTML = "<thead><tr><th>Tags</th><th>ID</th><th>Size</th><th></th></tr></thead>";
      const body = document.createElement("tbody");
      data.forEach((img) => {
        const tr = document.createElement("tr");
        const tags = (img.repo_tags || img.RepoTags || []).join(", ");
        const id = img.id || img.Id || "";
        tr.innerHTML = "<td>" + tags + "</td><td><code>" + String(id).slice(0, 19) + "</code></td><td>" +
          (img.size || img.Size || "") + "</td>";
        const td = document.createElement("td");
        td.appendChild(actionForm(node, "image_remove", { name: tags.split(",")[0] || id }));
        tr.appendChild(td);
        body.appendChild(tr);
      });
      table.appendChild(body);
      images.replaceChildren(table);
    }
  }

  const volumes = document.getElementById("volumes");
  if (volumes) {
    const data = parse(volumes);
    const node = volumes.getAttribute("data-node");
    const list = (data && data.volumes) || (data && data.Volumes) || [];
    const table = document.createElement("table");
    table.innerHTML = "<thead><tr><th>Name</th><th>Driver</th><th></th></tr></thead>";
    const body = document.createElement("tbody");
    (Array.isArray(list) ? list : []).forEach((v) => {
      const tr = document.createElement("tr");
      const name = v.name || v.Name || "";
      tr.innerHTML = "<td>" + name + "</td><td>" + (v.driver || v.Driver || "") + "</td>";
      const td = document.createElement("td");
      td.appendChild(actionForm(node, "volume_remove", { name: name }));
      tr.appendChild(td);
      body.appendChild(tr);
    });
    table.appendChild(body);
    volumes.replaceChildren(table);
  }

  const networks = document.getElementById("networks");
  if (networks) {
    const data = parse(networks);
    const node = networks.getAttribute("data-node");
    if (Array.isArray(data)) {
      const table = document.createElement("table");
      table.innerHTML = "<thead><tr><th>Name</th><th>ID</th><th>Driver</th><th></th></tr></thead>";
      const body = document.createElement("tbody");
      data.forEach((n) => {
        const tr = document.createElement("tr");
        const id = n.id || n.Id || "";
        const name = n.name || n.Name || "";
        tr.innerHTML = "<td>" + name + "</td><td><code>" + String(id).slice(0, 12) + "</code></td><td>" +
          (n.driver || n.Driver || "") + "</td>";
        const td = document.createElement("td");
        td.appendChild(actionForm(node, "network_remove", { id: id }));
        tr.appendChild(td);
        body.appendChild(tr);
      });
      table.appendChild(body);
      networks.replaceChildren(table);
    }
  }

  function el(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  function donut(ratio) {
    const r = 36;
    const c = 2 * Math.PI * r;
    const p = Math.max(0, Math.min(1, Number(ratio) || 0));
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 100 100");
    const bg = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    bg.setAttribute("cx", "50");
    bg.setAttribute("cy", "50");
    bg.setAttribute("r", String(r));
    bg.setAttribute("fill", "none");
    bg.setAttribute("stroke", "var(--line)");
    bg.setAttribute("stroke-width", "10");
    const fg = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    fg.setAttribute("cx", "50");
    fg.setAttribute("cy", "50");
    fg.setAttribute("r", String(r));
    fg.setAttribute("fill", "none");
    fg.setAttribute("stroke-width", "10");
    fg.setAttribute("stroke-linecap", "round");
    fg.setAttribute("stroke-dasharray", (p * c) + " " + c);
    fg.setAttribute("transform", "rotate(-90 50 50)");
    fg.setAttribute("class", "fill");
    const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
    label.setAttribute("x", "50");
    label.setAttribute("y", "55");
    label.setAttribute("text-anchor", "middle");
    label.setAttribute("fill", "currentColor");
    label.setAttribute("font-size", "18");
    label.setAttribute("font-weight", "650");
    label.textContent = Math.round(p * 100) + "%";
    svg.appendChild(bg);
    svg.appendChild(fg);
    svg.appendChild(label);
    return svg;
  }

  function sparkline(points) {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 240 64");
    svg.setAttribute("class", "spark");
    svg.setAttribute("preserveAspectRatio", "none");
    if (!points || points.length < 2) {
      return svg;
    }
    const vs = points.map((p) => p.v);
    const min = Math.min.apply(null, vs);
    const max = Math.max.apply(null, vs);
    const span = (max - min) || 1;
    const pad = 4;
    const d = points.map((p, i) => {
      const x = pad + i * (240 - 2 * pad) / Math.max(points.length - 1, 1);
      const y = 64 - pad - ((p.v - min) / span) * (64 - 2 * pad);
      return (i ? "L" : "M") + x.toFixed(1) + " " + y.toFixed(1);
    }).join(" ");
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    svg.appendChild(path);
    return svg;
  }

  function renderWidget(w) {
    const card = el("article", "widget span-" + (w.span || 1) + (w.tone ? " tone-" + w.tone : ""));
    card.appendChild(el("h3", null, w.title || w.id));
    const empty = w.display === "—" && !(w.rows && w.rows.length) && !(w.spark && w.spark.length);
    if (empty) card.classList.add("empty");
    if (w.kind === "gauge") {
      if (w.ratio == null) {
        card.classList.add("empty");
        card.appendChild(el("div", "stat", "—"));
      } else {
        const wrap = el("div", "gauge-wrap");
        wrap.appendChild(donut(w.ratio));
        wrap.appendChild(el("div", "gauge-caption", w.display || "—"));
        card.appendChild(wrap);
      }
    } else if (w.kind === "bar_list") {
      (w.rows || []).forEach((row) => {
        const block = el("div", "bar-row" + (row.ratio >= 0.9 ? " tone-crit" : row.ratio >= 0.75 ? " tone-warn" : " tone-ok"));
        const meta = el("div", "bar-meta");
        meta.appendChild(el("span", null, row.label || ""));
        meta.appendChild(el("span", null, row.display || ""));
        block.appendChild(meta);
        const track = el("div", "bar-track");
        const fill = el("div", "bar-fill");
        fill.style.width = Math.round((Number(row.ratio) || 0) * 100) + "%";
        track.appendChild(fill);
        block.appendChild(track);
        card.appendChild(block);
      });
      if (!(w.rows && w.rows.length)) card.appendChild(el("p", "muted", "No series yet."));
    } else if (w.kind === "sparkline") {
      card.appendChild(el("div", "stat", w.display || "—"));
      card.appendChild(sparkline(w.spark || []));
      (w.rows || []).forEach((row) => {
        const meta = el("div", "bar-meta");
        meta.appendChild(el("span", null, row.label || ""));
        meta.appendChild(el("span", null, row.display || ""));
        card.appendChild(meta);
      });
    } else {
      card.appendChild(el("div", "stat", w.display || "—"));
    }
    return card;
  }

  function parseAttr(node, name) {
    try {
      return JSON.parse(node.getAttribute(name) || "null");
    } catch (e) {
      return null;
    }
  }

  function clone(v) {
    return JSON.parse(JSON.stringify(v));
  }

  const widgetsHost = document.getElementById("overview-widgets");
  if (widgetsHost) {
    const nodeId = widgetsHost.getAttribute("data-node");
    const pollSecs = Math.max(1, Math.min(60, Number(widgetsHost.getAttribute("data-poll-secs") || 1) || 1));
    const toolbar = document.getElementById("overview-toolbar");
    const presets = parseAttr(widgetsHost, "data-presets") || [];
    let layout = parseAttr(widgetsHost, "data-layout") || { version: 1, widgets: [] };
    let source = widgetsHost.getAttribute("data-source") || "default";
    let hydrated = parse(widgetsHost);
    let editing = false;
    let backup = null;
    const dashUrl = "/api/v1/nodes/" + encodeURIComponent(nodeId) + "/dashboard";

    function paintWidgets() {
      const byId = {};
      (Array.isArray(hydrated) ? hydrated : []).forEach((w) => {
        byId[w.id] = w;
      });
      const grid = el("div", "widget-grid");
      const items = (layout && layout.widgets) || [];
      if (!items.length) {
        grid.appendChild(el("p", "muted", "No widgets yet. Use Customize to add some."));
      }
      items.forEach((spec, idx) => {
        const h = Object.assign({
          id: spec.id,
          kind: spec.kind,
          title: spec.title,
          span: spec.span,
          display: "—",
          tone: "",
          rows: [],
          spark: []
        }, byId[spec.id] || {});
        h.title = spec.title;
        h.span = spec.span;
        h.kind = spec.kind;
        const card = renderWidget(h);
        if (editing) {
          card.classList.add("editing");
          card.insertBefore(editChrome(idx), card.firstChild);
        }
        grid.appendChild(card);
      });
      widgetsHost.replaceChildren(grid);
    }

    function editChrome(idx) {
      const row = el("div", "widget-edit");
      const n = layout.widgets.length;
      const span = Number(layout.widgets[idx].span) || 1;
      function addBtn(label, fn, disabled) {
        const b = el("button", null, label);
        b.type = "button";
        b.disabled = !!disabled;
        b.addEventListener("click", (ev) => {
          ev.preventDefault();
          fn();
        });
        row.appendChild(b);
      }
      addBtn("↑", () => moveWidget(idx, -1), idx === 0);
      addBtn("↓", () => moveWidget(idx, 1), idx >= n - 1);
      addBtn("−", () => setSpan(idx, span - 1), span <= 1);
      addBtn("+", () => setSpan(idx, span + 1), span >= 4);
      addBtn("Remove", () => {
        layout.widgets.splice(idx, 1);
        paintAll();
      });
      return row;
    }

    function moveWidget(idx, dir) {
      const j = idx + dir;
      if (j < 0 || j >= layout.widgets.length) return;
      const w = layout.widgets[idx];
      layout.widgets[idx] = layout.widgets[j];
      layout.widgets[j] = w;
      paintAll();
    }

    function setSpan(idx, span) {
      layout.widgets[idx].span = Math.max(1, Math.min(4, span));
      paintAll();
    }

    function uniqueId(base) {
      const used = {};
      (layout.widgets || []).forEach((w) => { used[w.id] = true; });
      if (!used[base]) return base;
      let n = 2;
      while (used[base + "-" + n]) n += 1;
      return base + "-" + n;
    }

    function addPreset(id) {
      const p = presets.find((x) => x.id === id);
      if (!p || !p.widget) return;
      const w = clone(p.widget);
      w.id = uniqueId(p.widget.id || p.id);
      layout.widgets.push(w);
      paintAll();
    }

    function paintToolbar() {
      if (!toolbar) return;
      toolbar.replaceChildren();
      if (!editing) {
        const b = el("button", null, "Customize");
        b.type = "button";
        b.addEventListener("click", enterEdit);
        toolbar.appendChild(b);
        toolbar.appendChild(el("span", "muted", source === "custom" ? "Custom layout" : "Default layout"));
        return;
      }
      const select = document.createElement("select");
      select.appendChild(new Option("Add widget…", ""));
      const groups = {};
      presets.forEach((p) => {
        (groups[p.group] || (groups[p.group] = [])).push(p);
      });
      Object.keys(groups).sort().forEach((g) => {
        const og = document.createElement("optgroup");
        og.label = g;
        groups[g].forEach((p) => {
          og.appendChild(new Option((p.widget && p.widget.title ? p.widget.title : p.id) + " — " + (p.description || ""), p.id));
        });
        select.appendChild(og);
      });
      select.addEventListener("change", () => {
        if (!select.value) return;
        addPreset(select.value);
        select.value = "";
      });
      toolbar.appendChild(select);
      const save = el("button", null, "Save");
      save.type = "button";
      save.addEventListener("click", saveLayout);
      toolbar.appendChild(save);
      const cancel = el("button", null, "Cancel");
      cancel.type = "button";
      cancel.addEventListener("click", cancelEdit);
      toolbar.appendChild(cancel);
      const reset = el("button", null, "Reset to default");
      reset.type = "button";
      reset.addEventListener("click", resetLayout);
      toolbar.appendChild(reset);
    }

    function paintAll() {
      paintToolbar();
      paintWidgets();
    }

    function enterEdit() {
      editing = true;
      backup = clone(layout);
      paintAll();
    }

    function cancelEdit() {
      editing = false;
      if (backup) layout = backup;
      backup = null;
      paintAll();
    }

    async function saveLayout() {
      try {
        const r = await fetch(dashUrl, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(layout)
        });
        if (!r.ok) {
          window.alert("Could not save layout: " + (await r.text()));
          return;
        }
        editing = false;
        backup = null;
        source = "custom";
        await refresh();
      } catch (e) {
        window.alert("Could not save layout.");
      }
    }

    async function resetLayout() {
      if (!window.confirm("Reset this node’s dashboard to the built-in default?")) return;
      try {
        const r = await fetch(dashUrl, { method: "DELETE" });
        if (!r.ok) {
          window.alert("Could not reset layout: " + (await r.text()));
          return;
        }
        editing = false;
        backup = null;
        source = "default";
        await refresh();
      } catch (e) {
        window.alert("Could not reset layout.");
      }
    }

    async function refresh() {
      try {
        const r = await fetch(dashUrl);
        if (r.status === 401 || r.status === 403) return "auth";
        if (!r.ok) return;
        const j = await r.json();
        hydrated = j.widgets;
        if (!editing) {
          if (j.layout) layout = j.layout;
          if (j.source) source = j.source;
          paintAll();
        } else {
          paintWidgets();
        }
      } catch (e) {}
    }

    paintAll();
    if (nodeId) {
      let inflight = false;
      const timer = setInterval(async () => {
        if (document.hidden || inflight) return;
        inflight = true;
        try {
          const status = await refresh();
          if (status === "auth") clearInterval(timer);
        } finally {
          inflight = false;
        }
      }, pollSecs * 1000);
    }
  }
})();
