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

  const navCount = document.getElementById("nav-alert-count");
  const alertsTable = document.getElementById("alerts");

  function paintAlertCount(n) {
    if (!navCount) return;
    const c = Number(n) || 0;
    navCount.textContent = c > 0 ? String(c) : "";
    navCount.classList.toggle("is-idle", c === 0);
  }

  function paintAlertsPage(alerts) {
    if (!alertsTable) return;
    const tb = alertsTable.querySelector("tbody");
    if (!tb) return;
    tb.replaceChildren();
    (alerts || []).forEach((a) => {
      const tr = document.createElement("tr");
      tr.setAttribute("data-node", a.node_id || "");
      tr.setAttribute("data-chip", a.chip || "");
      const host = document.createElement("td");
      const link = document.createElement("a");
      link.href = "/nodes/" + encodeURIComponent(a.node_id || "");
      link.textContent = a.hostname || a.node_id || "";
      const sub = document.createElement("div");
      sub.className = "muted host-id";
      const code = document.createElement("code");
      code.textContent = a.node_id || "";
      sub.appendChild(code);
      host.appendChild(link);
      host.appendChild(sub);
      const label = document.createElement("td");
      label.textContent = a.label || "";
      const value = document.createElement("td");
      const chip = document.createElement("span");
      chip.className = "chip tone-" + (a.severity || "");
      chip.textContent = a.display || "";
      value.appendChild(chip);
      const hint = document.createElement("td");
      hint.className = "muted";
      hint.textContent = a.hint || "";
      tr.appendChild(host);
      tr.appendChild(label);
      tr.appendChild(value);
      tr.appendChild(hint);
      tb.appendChild(tr);
    });
    const empty = document.getElementById("alerts-empty");
    const n = (alerts || []).length;
    alertsTable.hidden = n === 0;
    if (empty) empty.hidden = n > 0;
  }

  async function refreshAlerts() {
    try {
      const r = await fetch("/api/v1/alerts");
      if (r.status === 401 || r.status === 403) return "auth";
      if (!r.ok) return;
      const j = await r.json();
      const list = j.alerts || [];
      paintAlertCount(list.length);
      paintAlertsPage(list);
    } catch (e) {}
  }

  if (navCount || alertsTable) {
    let alertsInflight = false;
    refreshAlerts();
    const alertsTimer = setInterval(async () => {
      if (document.hidden || alertsInflight) return;
      alertsInflight = true;
      try {
        if (await refreshAlerts() === "auth") clearInterval(alertsTimer);
      } finally {
        alertsInflight = false;
      }
    }, 2000);
  }

  const fleet = document.getElementById("fleet");
  if (fleet) {
    function paintFleet(nodes) {
      const byId = {};
      (nodes || []).forEach((n) => { byId[n.node_id] = n; });
      fleet.querySelectorAll("tbody tr[data-node]").forEach((tr) => {
        const id = tr.getAttribute("data-node");
        const n = byId[id];
        if (!n) return;
        (n.chips || []).forEach((c) => {
          const el = tr.querySelector('[data-chip="' + c.id + '"]');
          if (!el) return;
          el.textContent = c.display || "—";
          el.className = "chip tone-" + (c.tone || "");
          if (c.hint) el.setAttribute("title", c.hint);
          else el.removeAttribute("title");
        });
        const st = tr.querySelector("[data-status]");
        if (st) {
          st.textContent = n.status || "";
          st.className = "status status-" + String(n.status || "").replace(/ /g, "-");
        }
        const seen = tr.querySelector("[data-seen]");
        if (seen) seen.textContent = n.last_seen || "";
        const mark = tr.querySelector("[data-alert-count]");
        const ac = Number(n.alert_count) || 0;
        if (mark) {
          mark.textContent = ac > 0 ? String(ac) : "";
          mark.classList.toggle("is-idle", ac === 0);
        }
        tr.classList.toggle("has-alert", ac > 0);
      });
      let total = 0;
      (nodes || []).forEach((n) => { total += Number(n.alert_count) || 0; });
      paintAlertCount(total);
    }
    async function refreshFleet() {
      try {
        const r = await fetch("/api/v1/nodes");
        if (r.status === 401 || r.status === 403) return "auth";
        if (!r.ok) return;
        const j = await r.json();
        paintFleet(j.nodes);
      } catch (e) {}
    }
    let inflight = false;
    const timer = setInterval(async () => {
      if (document.hidden || inflight) return;
      inflight = true;
      try {
        if (await refreshFleet() === "auth") clearInterval(timer);
      } finally {
        inflight = false;
      }
    }, 1000);
  }

  function parse(host) {
    try {
      return JSON.parse(host.getAttribute("data-json") || "null");
    } catch (e) {
      return null;
    }
  }

  function el(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  function tdText(text) {
    const n = document.createElement("td");
    n.textContent = text == null ? "" : String(text);
    return n;
  }

  function tdCode(text) {
    const n = document.createElement("td");
    const c = document.createElement("code");
    c.textContent = text == null ? "" : String(text);
    n.appendChild(c);
    return n;
  }

  function thead(labels) {
    const head = document.createElement("thead");
    const tr = document.createElement("tr");
    labels.forEach((label) => {
      const th = document.createElement("th");
      th.textContent = label;
      tr.appendChild(th);
    });
    head.appendChild(tr);
    return head;
  }

  function formatBytes(n) {
    const x = Number(n);
    if (!Number.isFinite(x) || x < 0) return "";
    const units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let v = x;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i += 1;
    }
    return (i === 0 ? String(Math.round(v)) : v.toFixed(1)) + " " + units[i];
  }

  const OP_LABELS = {
    container_start: "Start",
    container_stop: "Stop",
    container_restart: "Restart",
    container_kill: "Kill",
    container_remove: "Remove",
    compose_up: "Up",
    compose_down: "Down",
    compose_pull: "Pull",
    image_remove: "Remove",
    volume_remove: "Remove",
    network_remove: "Remove"
  };

  const CONFIRM = {
    container_kill: "Kill this container?",
    container_remove: "Remove this container? This cannot be undone.",
    compose_down: "Compose down this project?",
    image_remove: "Remove this image?",
    image_prune: "Prune unused images on this node?",
    volume_remove: "Remove this volume?",
    network_remove: "Remove this network?"
  };

  function manageOn(host) {
    return host.getAttribute("data-manage") === "1";
  }

  function dockerBlocked(host) {
    const reason = host.getAttribute("data-reason") || "";
    if (reason === "offline") {
      host.replaceChildren(el("p", "muted", "Agent is not connected. Docker commands need a live session."));
      return true;
    }
    if (reason === "disabled") {
      host.replaceChildren(el("p", "muted", "Observe Docker is off. Enable it on the Settings tab."));
      return true;
    }
    return false;
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
    b.textContent = OP_LABELS[op] || op;
    if (CONFIRM[op]) b.className = "danger";
    f.appendChild(b);
    const msg = CONFIRM[op];
    if (msg) {
      f.addEventListener("submit", (ev) => {
        if (!window.confirm(msg)) ev.preventDefault();
      });
    }
    return f;
  }

  function actionsCell(children) {
    const td = document.createElement("td");
    td.className = "actions";
    children.forEach((n) => td.appendChild(n));
    return td;
  }

  function logsLink(href) {
    const a = document.createElement("a");
    a.href = href;
    a.textContent = "Logs";
    return a;
  }

  function stateCell(state, status) {
    const td = document.createElement("td");
    const s = (state || "").toLowerCase();
    const span = el("span", "state state-" + (s || "unknown"), (state || "") + (status ? " · " + status : ""));
    td.appendChild(span);
    return td;
  }

  function emptyRow(cols, text) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = cols;
    td.className = "muted";
    td.textContent = text;
    tr.appendChild(td);
    return tr;
  }

  document.querySelectorAll("form[data-confirm]").forEach((f) => {
    f.addEventListener("submit", (ev) => {
      if (!window.confirm(f.getAttribute("data-confirm"))) ev.preventDefault();
    });
  });

  const logView = document.getElementById("log-view");
  if (logView) {
    const url = logView.getAttribute("data-stream");
    if (url) {
      const es = new EventSource(url);
      function appendLog(text) {
        logView.textContent += text;
        if (logView.textContent.length > 400000) {
          logView.textContent = logView.textContent.slice(-300000);
        }
        logView.scrollTop = logView.scrollHeight;
      }
      es.onmessage = (e) => {
        try {
          const j = JSON.parse(e.data);
          appendLog(j.t || "");
        } catch (_) {
          appendLog(e.data);
        }
      };
      es.addEventListener("done", () => {
        es.close();
        appendLog("\n— end —\n");
      });
      window.addEventListener("beforeunload", () => es.close());
    }
  }

  const containers = document.getElementById("containers");
  if (containers && !dockerBlocked(containers)) {
    const data = parse(containers);
    const node = containers.getAttribute("data-node");
    const table = document.createElement("table");
    table.appendChild(thead(["Name", "Image", "State", "Project", ""]));
    const body = document.createElement("tbody");
    if (!Array.isArray(data) || !data.length) {
      body.appendChild(emptyRow(5, "No containers."));
    } else {
      data.forEach((c) => {
        const tr = document.createElement("tr");
        const name = (c.names && c.names[0]) ? c.names[0].replace(/^\//, "") : (c.id || "");
        const id = c.id_full || c.id || "";
        const nameTd = document.createElement("td");
        nameTd.appendChild(document.createTextNode(name));
        nameTd.appendChild(document.createElement("br"));
        const code = document.createElement("code");
        code.textContent = c.id || "";
        nameTd.appendChild(code);
        tr.appendChild(nameTd);
        tr.appendChild(tdText(c.image || ""));
        tr.appendChild(stateCell(c.state, c.status));
        tr.appendChild(tdText(c.compose_project || ""));
        const acts = [];
        if (manageOn(containers)) {
          ["container_start", "container_stop", "container_restart", "container_kill", "container_remove"].forEach((op) => {
            acts.push(actionForm(node, op, { id: id }));
          });
        }
        acts.push(logsLink("/nodes/" + encodeURIComponent(node) + "/containers/" + encodeURIComponent(id) + "/logs"));
        tr.appendChild(actionsCell(acts));
        body.appendChild(tr);
      });
    }
    table.appendChild(body);
    containers.replaceChildren(table);
  }

  const compose = document.getElementById("compose");
  if (compose && !dockerBlocked(compose)) {
    const data = parse(compose);
    const node = compose.getAttribute("data-node");
    const wrap = document.createElement("div");
    const projects = (data && typeof data === "object" && !Array.isArray(data)) ? Object.keys(data) : [];
    if (!projects.length) {
      wrap.appendChild(el("p", "muted", "No Compose projects. Set Compose files on Settings, or start a stack that sets com.docker.compose.project."));
    } else {
      projects.forEach((project) => {
        const head = el("div", "compose-head");
        head.appendChild(el("h3", null, project));
        const tools = el("div", "actions");
        if (manageOn(compose)) {
          tools.appendChild(actionForm(node, "compose_up", { project: project }));
          tools.appendChild(actionForm(node, "compose_down", { project: project }));
          tools.appendChild(actionForm(node, "compose_pull", { project: project }));
        }
        tools.appendChild(logsLink("/nodes/" + encodeURIComponent(node) + "/compose/" + encodeURIComponent(project) + "/logs"));
        head.appendChild(tools);
        wrap.appendChild(head);
        const table = document.createElement("table");
        table.appendChild(thead(["Service", "Name", "Image", "State"]));
        const body = document.createElement("tbody");
        const services = Array.isArray(data[project]) ? data[project] : [];
        if (!services.length) {
          body.appendChild(emptyRow(4, "No services."));
        } else {
          services.forEach((s) => {
            const tr = document.createElement("tr");
            tr.appendChild(tdText(s.service || ""));
            tr.appendChild(tdText(s.name || ""));
            tr.appendChild(tdText(s.image || ""));
            tr.appendChild(stateCell(s.state, s.status));
            body.appendChild(tr);
          });
        }
        table.appendChild(body);
        wrap.appendChild(table);
      });
    }
    compose.replaceChildren(wrap);
  }

  const images = document.getElementById("images");
  if (images && !dockerBlocked(images)) {
    const data = parse(images);
    const node = images.getAttribute("data-node");
    const table = document.createElement("table");
    table.appendChild(thead(["Tags", "ID", "Size", ""]));
    const body = document.createElement("tbody");
    const rows = Array.isArray(data) ? data : [];
    if (!rows.length) {
      body.appendChild(emptyRow(4, "No images."));
    } else {
      rows.forEach((img) => {
        const tr = document.createElement("tr");
        const tags = img.tags || img.repo_tags || [];
        const tagText = Array.isArray(tags) ? tags.join(", ") : "";
        const id = img.id_short || img.id || "";
        tr.appendChild(tdText(tagText || "<none>"));
        tr.appendChild(tdCode(id));
        tr.appendChild(tdText(formatBytes(img.size)));
        const acts = [];
        if (manageOn(images)) {
          const name = (Array.isArray(tags) && tags[0]) ? tags[0] : (img.id || "");
          acts.push(actionForm(node, "image_remove", { name: name }));
        }
        tr.appendChild(actionsCell(acts));
        body.appendChild(tr);
      });
    }
    table.appendChild(body);
    images.replaceChildren(table);
  }

  const volumes = document.getElementById("volumes");
  if (volumes && !dockerBlocked(volumes)) {
    const data = parse(volumes);
    const node = volumes.getAttribute("data-node");
    const table = document.createElement("table");
    table.appendChild(thead(["Name", "Driver", "Mountpoint", ""]));
    const body = document.createElement("tbody");
    const list = Array.isArray(data) ? data : ((data && data.volumes) || []);
    if (!list.length) {
      body.appendChild(emptyRow(4, "No volumes."));
    } else {
      list.forEach((v) => {
        const tr = document.createElement("tr");
        const name = v.name || "";
        tr.appendChild(tdText(name));
        tr.appendChild(tdText(v.driver || ""));
        tr.appendChild(tdCode(v.mountpoint || ""));
        const acts = [];
        if (manageOn(volumes)) {
          acts.push(actionForm(node, "volume_remove", { name: name }));
        }
        tr.appendChild(actionsCell(acts));
        body.appendChild(tr);
      });
    }
    table.appendChild(body);
    volumes.replaceChildren(table);
  }

  const networks = document.getElementById("networks");
  if (networks && !dockerBlocked(networks)) {
    const data = parse(networks);
    const node = networks.getAttribute("data-node");
    const table = document.createElement("table");
    table.appendChild(thead(["Name", "ID", "Driver", "Scope", ""]));
    const body = document.createElement("tbody");
    const rows = Array.isArray(data) ? data : [];
    if (!rows.length) {
      body.appendChild(emptyRow(5, "No networks."));
    } else {
      rows.forEach((n) => {
        const tr = document.createElement("tr");
        const id = n.id || "";
        tr.appendChild(tdText(n.name || ""));
        tr.appendChild(tdCode(n.id_short || id.slice(0, 12)));
        tr.appendChild(tdText(n.driver || ""));
        tr.appendChild(tdText(n.scope || ""));
        const acts = [];
        if (manageOn(networks)) {
          acts.push(actionForm(node, "network_remove", { id: id }));
        }
        tr.appendChild(actionsCell(acts));
        body.appendChild(tr);
      });
    }
    table.appendChild(body);
    networks.replaceChildren(table);
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
          bindDrag(card, idx);
        }
        grid.appendChild(card);
      });
      widgetsHost.replaceChildren(grid);
    }

    function bindDrag(card, idx) {
      card.draggable = true;
      card.addEventListener("dragstart", (e) => {
        e.dataTransfer.setData("text/plain", String(idx));
        e.dataTransfer.effectAllowed = "move";
        card.classList.add("dragging");
      });
      card.addEventListener("dragend", () => card.classList.remove("dragging"));
      card.addEventListener("dragover", (e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        card.classList.add("drop-target");
      });
      card.addEventListener("dragleave", () => card.classList.remove("drop-target"));
      card.addEventListener("drop", (e) => {
        e.preventDefault();
        e.stopPropagation();
        card.classList.remove("drop-target");
        const from = Number(e.dataTransfer.getData("text/plain"));
        if (!Number.isFinite(from)) return;
        moveWidgetTo(from, idx);
      });
      card.querySelectorAll("button").forEach((b) => {
        b.addEventListener("mousedown", (ev) => ev.stopPropagation());
        b.addEventListener("dragstart", (ev) => ev.preventDefault());
      });
    }

    function moveWidgetTo(from, to) {
      if (from === to || from < 0 || to < 0) return;
      const n = layout.widgets.length;
      if (from >= n || to >= n) return;
      const item = layout.widgets.splice(from, 1)[0];
      let insert = to;
      if (from < to) insert = to - 1;
      layout.widgets.splice(insert, 0, item);
      paintAll();
    }

    function editChrome(idx) {
      const row = el("div", "widget-edit");
      const n = layout.widgets.length;
      const span = Number(layout.widgets[idx].span) || 1;
      const grip = el("span", "widget-grip", "⋮⋮");
      grip.title = "Drag to move";
      grip.setAttribute("aria-hidden", "true");
      row.appendChild(grip);
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
      toolbar.appendChild(el("span", "muted", "Drag cards to place them. +/− changes width."));
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

  const TOUR_KEY = "keystone.tour.v1";
  const replayTour = document.getElementById("replay-tour");
  if (replayTour) {
    replayTour.addEventListener("click", () => {
      try { localStorage.removeItem(TOUR_KEY); } catch (e) {}
      location.href = "/?welcome=1";
    });
  }

  function tourDone() {
    try { localStorage.setItem(TOUR_KEY, "done"); } catch (e) {}
    const u = new URL(location.href);
    if (u.searchParams.has("welcome")) {
      u.searchParams.delete("welcome");
      const next = u.pathname + u.search + u.hash;
      history.replaceState({}, "", next || "/");
    }
  }

  function tourSeen() {
    try { return localStorage.getItem(TOUR_KEY) === "done"; } catch (e) { return false; }
  }

  function wantTour() {
    try {
      if (new URLSearchParams(location.search).get("welcome") === "1") return true;
    } catch (e) {}
    return !tourSeen();
  }

  function startTour() {
    const steps = [
      {
        sel: "a.brand",
        title: "Welcome to KeyStone",
        body: "One console for the lab: live host metrics and per-node Docker. This short tour points at the header."
      },
      {
        sel: "nav a[href='/']",
        title: "Nodes",
        body: "The home page is the fleet: CPU, RAM, disk, and temperature for every host, about once a second."
      },
      {
        sel: "#nav-alerts",
        title: "Alerts",
        body: "Warn and crit chips (75% / 90%, 75°C / 90°C) land here. Optional webhook is on Settings."
      },
      {
        sel: "nav a[href='/settings']",
        title: "Settings",
        body: "Retention, ingest token, scrape jobs, alert webhook, password, and authenticator 2FA. Turn on 2FA before exposing this UI through a tunnel."
      },
      {
        sel: "a.btn[href='/nodes/new']",
        title: "Add a node",
        body: "Enroll a hostname, then install the agent on that machine. Open a host for Overview — Customize, then drag cards to place them."
      }
    ].filter((s) => document.querySelector(s.sel));
    if (!steps.length) {
      return;
    }
    let i = 0;
    const backdrop = el("div", "tour-backdrop");
    const spot = el("div", "tour-spot");
    const card = el("div", "tour-card");
    backdrop.setAttribute("role", "dialog");
    backdrop.setAttribute("aria-label", "Welcome tour");
    function place() {
      const step = steps[i];
      const target = document.querySelector(step.sel);
      card.replaceChildren();
      card.appendChild(el("h2", null, step.title));
      card.appendChild(el("p", null, step.body));
      const nav = el("div", "tour-actions");
      const skip = el("button", null, "Skip");
      skip.type = "button";
      skip.addEventListener("click", close);
      const next = el("button", null, i === steps.length - 1 ? "Done" : "Next");
      next.type = "button";
      next.addEventListener("click", () => {
        if (i >= steps.length - 1) close();
        else {
          i += 1;
          place();
        }
      });
      nav.appendChild(skip);
      nav.appendChild(next);
      card.appendChild(nav);
      if (!target) {
        spot.style.display = "none";
        card.style.top = "20%";
        card.style.left = "50%";
        card.style.transform = "translateX(-50%)";
        return;
      }
      const r = target.getBoundingClientRect();
      const pad = 6;
      spot.style.display = "block";
      spot.style.top = Math.max(0, r.top - pad) + "px";
      spot.style.left = Math.max(0, r.left - pad) + "px";
      spot.style.width = r.width + pad * 2 + "px";
      spot.style.height = r.height + pad * 2 + "px";
      const below = r.bottom + 12;
      const above = r.top - 12;
      card.style.transform = "none";
      if (below + 180 < window.innerHeight) {
        card.style.top = below + "px";
      } else {
        card.style.top = Math.max(12, above - 160) + "px";
      }
      const left = Math.min(Math.max(12, r.left), window.innerWidth - 340);
      card.style.left = left + "px";
    }
    function close() {
      tourDone();
      backdrop.remove();
      window.removeEventListener("resize", place);
      document.removeEventListener("keydown", onKey);
    }
    function onKey(e) {
      if (e.key === "Escape") close();
    }
    backdrop.appendChild(spot);
    backdrop.appendChild(card);
    document.body.appendChild(backdrop);
    window.addEventListener("resize", place);
    document.addEventListener("keydown", onKey);
    place();
    setTimeout(() => {
      backdrop.addEventListener("click", (e) => {
        if (e.target === backdrop) close();
      });
    }, 400);
  }

  if (document.querySelector("header.bar") && wantTour()) {
    setTimeout(startTour, 80);
  }
})();
