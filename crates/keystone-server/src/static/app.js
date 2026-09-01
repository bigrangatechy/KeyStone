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

  function formatCpuRatio(n) {
    const x = Number(n);
    if (!Number.isFinite(x)) return "—";
    return Math.round(x * 100) + "%";
  }

  const OP_LABELS = {
    container_start: "Start",
    container_stop: "Stop",
    container_restart: "Restart",
    container_pause: "Pause",
    container_unpause: "Resume",
    container_kill: "Kill",
    container_remove: "Remove",
    compose_up: "Up",
    compose_start: "Start",
    compose_stop: "Stop",
    compose_restart: "Restart",
    compose_down: "Down",
    compose_pull: "Pull",
    compose_update: "Update",
    image_remove: "Remove",
    volume_remove: "Remove",
    network_remove: "Remove"
  };

  const CONFIRM = {
    container_kill: "Kill this container?",
    container_remove: "Remove this container? This cannot be undone.",
    container_prune: "Remove all stopped containers on this node?",
    compose_down: "Compose down removes this project's containers (not the Compose file). The project stays on this tab so you can Up it again. Continue?",
    compose_update: "Pull images and recreate this Compose project?",
    image_remove: "Remove this image?",
    image_prune: "Prune unused images on this node?",
    volume_remove: "Remove this volume?",
    volume_prune: "Remove unused volumes? This cannot be undone.",
    network_remove: "Remove this network?",
    network_prune: "Remove unused networks on this node?"
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
    const board = el("div", "container-board");
    const grid = el("div", "container-grid");
    const detail = el("div", "container-detail");
    detail.hidden = true;
    let selectedId = "";

    function containerDisplayName(c) {
      if (c.names && c.names[0]) return String(c.names[0]).replace(/^\//, "");
      return c.id || "";
    }

    function kvRow(label, value) {
      if (value == null || value === "") return null;
      const row = el("p", "detail-kv");
      row.appendChild(el("span", "muted", label));
      row.appendChild(document.createTextNode(" " + value));
      return row;
    }

    function showListDetail(c) {
      const name = containerDisplayName(c);
      const id = c.id_full || c.id || "";
      detail.hidden = false;
      board.classList.add("has-detail");
      detail.replaceChildren();
      detail.appendChild(el("h3", null, name || "Container"));
      const st = (c.state || "").toLowerCase();
      detail.appendChild(el("span", "state state-" + (st || "unknown"), (c.state || "") + (c.status ? " · " + c.status : "")));
      [
        kvRow("Id", c.id || ""),
        kvRow("Image", c.image || ""),
        kvRow("Ports", c.ports || ""),
        kvRow("Project", c.compose_project || "")
      ].forEach((n) => { if (n) detail.appendChild(n); });
      const inspectHost = el("div", "inspect-extra");
      inspectHost.appendChild(el("p", "muted", "Loading details…"));
      detail.appendChild(inspectHost);
      const acts = el("div", "actions");
      if (manageOn(containers)) {
        ["container_start", "container_stop", "container_restart", "container_pause", "container_unpause", "container_kill", "container_remove"].forEach((op) => {
          acts.appendChild(actionForm(node, op, { id: id }));
        });
      }
      acts.appendChild(logsLink("/nodes/" + encodeURIComponent(node) + "/containers/" + encodeURIComponent(id) + "/logs"));
      detail.appendChild(acts);
      return inspectHost;
    }

    function applyInspect(host, info) {
      host.replaceChildren();
      if (!info || typeof info !== "object") {
        host.appendChild(el("p", "muted", "Could not inspect this container."));
        return;
      }
      if (info.privileged) {
        host.appendChild(el("p", "error", "This container is privileged."));
      }
      if (info.error) {
        host.appendChild(el("p", "error", String(info.error)));
      }
      [
        kvRow("Service", info.compose_service || ""),
        kvRow("Created", info.created || ""),
        kvRow("Started", info.started_at || ""),
        kvRow("Pid", info.pid != null ? String(info.pid) : ""),
        kvRow("Restart", info.restart || ""),
        kvRow("Network mode", info.network_mode || ""),
        kvRow("Command", Array.isArray(info.command) ? info.command.join(" ") : "")
      ].forEach((n) => { if (n) host.appendChild(n); });
      const nets = Array.isArray(info.networks) ? info.networks : [];
      if (nets.length) {
        host.appendChild(el("p", "muted", "Networks"));
        nets.forEach((n) => {
          host.appendChild(el("p", null, (n.name || "") + (n.ip ? " · " + n.ip : "")));
        });
      }
      const mounts = Array.isArray(info.mounts) ? info.mounts : [];
      if (mounts.length) {
        const mt = document.createElement("table");
        mt.appendChild(thead(["Mount", "Source"]));
        const mb = document.createElement("tbody");
        mounts.forEach((m) => {
          const tr = document.createElement("tr");
          tr.appendChild(tdCode(m.destination || ""));
          tr.appendChild(tdText((m.source || "") + (m.rw === false ? " (ro)" : "")));
          mb.appendChild(tr);
        });
        mt.appendChild(mb);
        host.appendChild(mt);
      }
    }

    async function loadInspect(id, host) {
      try {
        const r = await fetch("/api/v1/nodes/" + encodeURIComponent(node) + "/containers/" + encodeURIComponent(id));
        const body = await r.json().catch(() => ({}));
        if (selectedId !== id) return;
        if (!r.ok) {
          host.replaceChildren(el("p", "muted", body.error || "Could not inspect this container."));
          return;
        }
        applyInspect(host, body);
      } catch (e) {
        if (selectedId !== id) return;
        host.replaceChildren(el("p", "muted", "Could not inspect this container."));
      }
    }

    function selectContainer(c) {
      const id = c.id_full || c.id || "";
      if (selectedId === id) {
        selectedId = "";
        detail.hidden = true;
        board.classList.remove("has-detail");
        grid.querySelectorAll(".container-card").forEach((card) => card.classList.remove("is-open"));
        return;
      }
      selectedId = id;
      grid.querySelectorAll(".container-card").forEach((card) => {
        card.classList.toggle("is-open", card.getAttribute("data-id-full") === id);
      });
      const extra = showListDetail(c);
      loadInspect(id, extra);
    }

    if (!Array.isArray(data) || !data.length) {
      board.appendChild(el("p", "muted", "No containers."));
    } else {
      data.forEach((c) => {
        const id = c.id_full || c.id || "";
        const card = document.createElement("button");
        card.type = "button";
        card.className = "container-card";
        if (c.id) card.setAttribute("data-id", c.id);
        card.setAttribute("data-id-full", id);
        card.appendChild(el("h3", null, containerDisplayName(c)));
        const st = (c.state || "").toLowerCase();
        card.appendChild(el("span", "state state-" + (st || "unknown"), c.state || "unknown"));
        const metrics = el("div", "container-metrics");
        metrics.appendChild(el("span", "usage-cpu", "CPU " + (formatCpuRatio(c.cpu_ratio) || "—")));
        metrics.appendChild(el("span", "usage-mem", "Mem " + (c.memory_bytes == null ? "—" : (formatBytes(c.memory_bytes) || "—"))));
        card.appendChild(metrics);
        card.addEventListener("click", () => selectContainer(c));
        grid.appendChild(card);
      });
      board.appendChild(grid);
      board.appendChild(detail);
    }
    containers.replaceChildren(board);

    function applyContainerUsage(map) {
      if (!map || typeof map !== "object") return;
      containers.querySelectorAll(".container-card[data-id]").forEach((card) => {
        const u = map[card.getAttribute("data-id")];
        if (!u) return;
        const cpu = card.querySelector(".usage-cpu");
        const mem = card.querySelector(".usage-mem");
        if (cpu && u.cpu_ratio != null) cpu.textContent = "CPU " + formatCpuRatio(u.cpu_ratio);
        if (mem && u.memory_bytes != null) mem.textContent = "Mem " + (formatBytes(u.memory_bytes) || "—");
      });
    }

    function containersVisible() {
      const panel = containers.closest("[data-panel]");
      return !document.hidden && (!panel || panel.classList.contains("active"));
    }

    async function refreshUsage() {
      if (!containersVisible()) return;
      try {
        const r = await fetch("/api/v1/nodes/" + encodeURIComponent(node) + "/container-usage");
        if (r.status === 401 || r.status === 403) return "auth";
        if (!r.ok) return;
        applyContainerUsage(await r.json());
      } catch (e) {}
    }

    const pollSecs = Math.max(1, Math.min(60, Number(containers.getAttribute("data-poll-secs") || 1) || 1));
    let usageInflight = false;
    const usageTimer = setInterval(async () => {
      if (usageInflight) return;
      usageInflight = true;
      try {
        const status = await refreshUsage();
        if (status === "auth") clearInterval(usageTimer);
      } finally {
        usageInflight = false;
      }
    }, pollSecs * 1000);
    document.querySelectorAll('[data-tab="containers"]').forEach((btn) => {
      btn.addEventListener("click", () => { refreshUsage(); });
    });
    refreshUsage();
  }

  const compose = document.getElementById("compose");
  if (compose && !dockerBlocked(compose)) {
    const data = parse(compose);
    const node = compose.getAttribute("data-node");
    const wrap = document.createElement("div");
    const projects = (data && typeof data === "object" && !Array.isArray(data)) ? Object.keys(data) : [];
    if (!projects.length) {
      wrap.appendChild(el("p", "muted", "No Compose projects. Set Compose files on Settings (paths the keystone user can read), or start a stack that sets com.docker.compose.project."));
    } else {
      projects.forEach((project) => {
        const head = el("div", "compose-head");
        head.appendChild(el("h3", null, project));
        const tools = el("div", "actions");
        if (manageOn(compose)) {
          tools.appendChild(actionForm(node, "compose_up", { project: project }));
          tools.appendChild(actionForm(node, "compose_start", { project: project }));
          tools.appendChild(actionForm(node, "compose_stop", { project: project }));
          tools.appendChild(actionForm(node, "compose_restart", { project: project }));
          tools.appendChild(actionForm(node, "compose_down", { project: project }));
          tools.appendChild(actionForm(node, "compose_pull", { project: project }));
          tools.appendChild(actionForm(node, "compose_update", { project: project }));
        }
        tools.appendChild(logsLink("/nodes/" + encodeURIComponent(node) + "/compose/" + encodeURIComponent(project) + "/logs"));
        head.appendChild(tools);
        wrap.appendChild(head);
        const table = document.createElement("table");
        table.appendChild(thead(["Service", "Name", "Image", "Ports", "State"]));
        const body = document.createElement("tbody");
        const services = Array.isArray(data[project]) ? data[project] : [];
        if (!services.length) {
          body.appendChild(emptyRow(5, "No containers (Compose down removes them). Start or Up brings them back."));
        } else {
          services.forEach((s) => {
            const tr = document.createElement("tr");
            tr.appendChild(tdText(s.service || ""));
            tr.appendChild(tdText(s.name || ""));
            tr.appendChild(tdText(s.image || ""));
            tr.appendChild(tdCode(s.ports || "—"));
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

  function hubError(r, data) {
    if (data && data.error) return data.error;
    if (r.status === 429) return "Docker Hub rate-limited this server. Type the image name to pull.";
    if (r.status === 401 || r.status === 403) return "Sign in again to search Docker Hub.";
    return "Could not reach Docker Hub. Type the image name to pull.";
  }

  function formatStars(n) {
    const x = Number(n);
    if (!Number.isFinite(x) || x <= 0) return "";
    if (x >= 1000) return Math.round(x / 1000) + "k stars";
    return String(Math.round(x)) + " stars";
  }

  function bindHubSearch() {
    const input = document.getElementById("hub-query");
    const out = document.getElementById("hub-results");
    const nameField = document.getElementById("image-pull-name");
    if (!input || !out || !nameField) return;

    let timer = 0;
    let searchAbort = null;
    let tagsAbort = null;

    function show(node) {
      out.hidden = !node;
      out.replaceChildren(node || document.createTextNode(""));
    }

    function statusLine(text, isError) {
      const p = el("p", isError ? "error" : "muted", text);
      const wrap = el("div", "hub-panel");
      wrap.appendChild(p);
      show(wrap);
    }

    input.addEventListener("input", () => {
      window.clearTimeout(timer);
      const q = input.value.trim();
      if (q.length < 2) {
        if (searchAbort) searchAbort.abort();
        if (tagsAbort) tagsAbort.abort();
        show(null);
        return;
      }
      timer = window.setTimeout(() => runSearch(q), 350);
    });

    async function runSearch(q) {
      if (searchAbort) searchAbort.abort();
      if (tagsAbort) tagsAbort.abort();
      searchAbort = new AbortController();
      statusLine("Searching Docker Hub…", false);
      try {
        const r = await fetch("/api/v1/dockerhub/search?q=" + encodeURIComponent(q), {
          signal: searchAbort.signal
        });
        const data = await r.json().catch(() => ({}));
        if (!r.ok) {
          statusLine(hubError(r, data), true);
          return;
        }
        paintRepos(Array.isArray(data.results) ? data.results : []);
      } catch (e) {
        if (e && e.name === "AbortError") return;
        statusLine("Could not reach Docker Hub. Type the image name to pull.", true);
      }
    }

    function paintRepos(results) {
      const wrap = el("div", "hub-panel");
      wrap.appendChild(el("p", "muted", "Pick a card, then a tag. That fills Pull — the agent still pulls. This is not an app store."));
      if (!results.length) {
        wrap.appendChild(el("p", "muted", "No repositories matched."));
        show(wrap);
        return;
      }
      const board = el("div", "hub-board");
      const grid = el("div", "hub-grid");
      const detail = el("div", "hub-detail");
      detail.hidden = true;
      let openName = "";
      results.forEach((repo) => {
        const pullName = repo.pull_name || repo.name || "";
        const card = document.createElement("button");
        card.type = "button";
        card.className = "hub-card";
        const title = document.createElement("h3");
        const code = document.createElement("code");
        code.textContent = pullName;
        title.appendChild(code);
        if (repo.official) {
          const badge = document.createElement("span");
          badge.className = "hub-official";
          badge.textContent = "official";
          title.appendChild(badge);
        }
        card.appendChild(title);
        if (repo.description) card.appendChild(el("p", "muted", repo.description));
        const stars = formatStars(repo.stars);
        if (stars) card.appendChild(el("span", "hub-stars", stars));
        card.addEventListener("click", () => {
          if (openName === pullName) {
            openName = "";
            card.classList.remove("is-open");
            board.classList.remove("has-detail");
            detail.hidden = true;
            detail.replaceChildren();
            return;
          }
          openName = pullName;
          grid.querySelectorAll(".hub-card").forEach((c) => c.classList.remove("is-open"));
          card.classList.add("is-open");
          board.classList.add("has-detail");
          detail.hidden = false;
          loadTags(detail, repo);
        });
        grid.appendChild(card);
      });
      board.appendChild(grid);
      board.appendChild(detail);
      wrap.appendChild(board);
      show(wrap);
    }

    async function loadTags(host, repo) {
      if (tagsAbort) tagsAbort.abort();
      tagsAbort = new AbortController();
      host.replaceChildren(el("p", "muted", "Loading tags…"));
      const ns = encodeURIComponent(repo.namespace || "library");
      const name = encodeURIComponent(repo.name || "");
      try {
        const r = await fetch("/api/v1/dockerhub/tags?namespace=" + ns + "&name=" + name, {
          signal: tagsAbort.signal
        });
        const data = await r.json().catch(() => ({}));
        if (!r.ok) {
          host.replaceChildren(el("p", "error", hubError(r, data)));
          return;
        }
        paintTags(host, repo, Array.isArray(data.results) ? data.results : []);
      } catch (e) {
        if (e && e.name === "AbortError") return;
        host.replaceChildren(el("p", "error", "Could not load tags. Type the image name to pull."));
      }
    }

    function paintTags(host, repo, results) {
      host.replaceChildren();
      host.appendChild(el("h3", null, repo.pull_name || repo.name || ""));
      host.appendChild(el("p", "muted", "A tag fills Pull (name, arch, last updated). You still press Pull."));
      if (!results.length) {
        host.appendChild(el("p", "muted", "No tags returned."));
        return;
      }
      const list = document.createElement("ul");
      list.className = "hub-tag-list";
      results.forEach((tag) => {
        const li = document.createElement("li");
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "hub-tag";
        const code = document.createElement("code");
        code.textContent = tag.pull_ref || "";
        btn.appendChild(code);
        const meta = document.createElement("span");
        meta.className = "hub-meta muted";
        const bits = [];
        if (tag.last_updated) bits.push(tag.last_updated);
        const archs = Array.isArray(tag.architectures) ? tag.architectures.join(", ") : "";
        if (archs) bits.push(archs);
        meta.textContent = bits.join(" · ");
        if (meta.textContent) btn.appendChild(meta);
        btn.addEventListener("click", () => {
          nameField.value = tag.pull_ref || "";
          nameField.focus();
        });
        li.appendChild(btn);
        list.appendChild(li);
      });
      host.appendChild(list);
    }
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

  bindHubSearch();

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

  const system = document.getElementById("system");
  if (system) {
    paintSystem(system);
  }

  function paintSystem(host) {
    const reason = host.getAttribute("data-reason") || "";
    const node = host.getAttribute("data-node") || "";
    const manage = host.getAttribute("data-manage") === "1";
    const totpOn = host.getAttribute("data-totp") === "1";
    const stepUpErr = new URLSearchParams(location.search).get("err");
    if (reason === "offline") {
      host.replaceChildren(el("p", "muted", "Agent is not connected. System commands need a live session."));
      return;
    }
    if (reason === "disabled") {
      host.replaceChildren(el("p", "muted", "System observe is off. On this node's Settings tab, enable Observe host updates and addressing. Enabling keystone-sys.socket alone is not enough."));
      return;
    }
    const data = parse(host) || {};
    const wrap = document.createElement("div");
    const helperOn = !!data.helper_running;
    if (data.helper_error) {
      wrap.appendChild(el("p", "error", String(data.helper_error)));
    }
    if (!helperOn) {
      wrap.appendChild(el("p", "muted", "System helper is not running. On this node:"));
      const pre = document.createElement("pre");
      pre.className = "snippet";
      pre.textContent = "sudo systemctl enable --now keystone-sys.socket";
      wrap.appendChild(pre);
      wrap.appendChild(el("p", "muted", "If that unit is already enabled, restart keystone-agent so it can use /run/keystone/sys.sock, then reload this tab. The metrics agent stays unprivileged."));
    }
    const split = el("div", "sys-split");
    const health = el("div", "sys-health");
    const actions = el("div", "sys-actions");
    health.appendChild(el("p", "sys-col-label", "Health"));
    actions.appendChild(el("p", "sys-col-label", "Actions"));
    const meta = el("p", "muted");
    const bits = [];
    if (data.hostname) bits.push(data.hostname);
    if (data.kernel) bits.push("kernel " + data.kernel);
    if (data.backend) bits.push(data.backend);
    meta.textContent = bits.join(" · ") || "No host snapshot yet.";
    health.appendChild(meta);
    const ntp = data.ntp || {};
    if (ntp.available) {
      const ntpLine = document.createElement("p");
      ntpLine.appendChild(el(
        "span",
        ntp.synchronized ? "chip tone-ok" : "chip tone-warn",
        ntp.synchronized ? "Clock synchronized" : "Clock not synchronized"
      ));
      health.appendChild(ntpLine);
    }
    const ssh = data.ssh || {};
    if (ssh.available) {
      const sshLine = document.createElement("p");
      sshLine.appendChild(el(
        "span",
        ssh.password_auth ? "chip tone-warn" : "chip tone-ok",
        ssh.password_auth ? "SSH password logins allowed" : "SSH keys only"
      ));
      health.appendChild(sshLine);
    }
    const uiHost = host.getAttribute("data-ui-host") === "1";
    if (data.reboot_required) {
      wrap.appendChild(el("p", "error", data.kernel_pending
        ? "Kernel update pending. Reboot this node when you are ready."
        : "Reboot required after package updates."));
    }
    if (helperOn) {
      const leftovers = Array.isArray(data.restart_services) ? data.restart_services : [];
      const failed = Array.isArray(data.failed_units) ? data.failed_units : [];
      if (manage && totpOn && (leftovers.length || failed.length)) {
        const totp = document.createElement("input");
        totp.id = "sys-restart-totp";
        totp.inputMode = "numeric";
        totp.autocomplete = "one-time-code";
        totp.maxLength = 6;
        totp.placeholder = "000000";
        const totpLab = document.createElement("label");
        totpLab.appendChild(document.createTextNode("Authenticator code for restart"));
        totpLab.appendChild(totp);
        health.appendChild(totpLab);
        health.appendChild(el("p", "muted", "Current 6-digit code. Backup codes are for sign-in only."));
      }
      health.appendChild(el("h3", null, "Services still using old libraries"));
      health.appendChild(el("p", "muted", "Listed by needrestart after upgrades. Restart runs systemctl restart for that name only — not a unit-name textbox."));
      if (!leftovers.length) {
        health.appendChild(el("p", "muted", "None right now."));
      } else {
        health.appendChild(unitNameTable(leftovers, { manage: manage, node: node, totpOn: totpOn, uiHost: uiHost }));
      }
      health.appendChild(el("h3", null, "Failed systemd units"));
      health.appendChild(el("p", "muted", "From systemctl --failed. Restart is the same listed-name button."));
      if (!failed.length) {
        health.appendChild(el("p", "muted", "None right now."));
      } else {
        health.appendChild(unitNameTable(failed, { manage: manage, node: node, totpOn: totpOn, uiHost: uiHost }));
      }
      health.appendChild(el("h3", null, "Journals"));
      health.appendChild(el("p", "muted", "Follow journalctl for these units. Leave the page to stop. Not a shell."));
      const journals = document.createElement("ul");
      [
        "keystone-agent.service",
        "keystone-server.service",
        "docker.service",
        "ssh.service",
        "gitlab-runsvdir.service"
      ].forEach((unit) => {
        const li = document.createElement("li");
        const a = document.createElement("a");
        a.href = "/nodes/" + encodeURIComponent(node) + "/sys/journal/" + encodeURIComponent(unit);
        a.textContent = unit;
        li.appendChild(a);
        journals.appendChild(li);
      });
      health.appendChild(journals);
      const unattended = data.unattended || {};
      health.appendChild(el("h3", null, "Unattended upgrades"));
      if (unattended.available) {
        const unLine = document.createElement("p");
        unLine.appendChild(el(
          "span",
          unattended.enabled ? "chip tone-ok" : "chip",
          unattended.enabled ? "enabled" : "off"
        ));
        health.appendChild(unLine);
        if (typeof unattended.last_unix === "number") {
          health.appendChild(el("p", "muted", "Last unattended run " + relativeUnix(unattended.last_unix) + "."));
        } else {
          health.appendChild(el("p", "muted", "No unattended run on disk"));
        }
        health.appendChild(el("p", "muted", "Glance only. This tab does not edit unattended-upgrades or turn it on."));
      } else {
        health.appendChild(el("p", "muted", "unattended-upgrades is not installed on this node."));
      }
    }
    if (manage && helperOn) {
      const rebootHead = el("div", "compose-head");
      rebootHead.appendChild(el("h3", null, "Reboot"));
      actions.appendChild(rebootHead);
      if (uiHost) {
        actions.appendChild(el("p", "error", "This node is serving the KeyStone UI. Rebooting it will take this session down until the server is back."));
      } else {
        actions.appendChild(el("p", "muted", "Hardcoded systemctl reboot. Poweroff is not in this UI. KeyStone units come back if they are enabled."));
      }
      const rebootForm = document.createElement("form");
      rebootForm.method = "post";
      rebootForm.action = "/nodes/" + encodeURIComponent(node) + "/sys/reboot";
      rebootForm.addEventListener("submit", (ev) => {
        const msg = uiHost
          ? "This node is serving the KeyStone UI. Rebooting it will take this session down until the server is back. Continue?"
          : "Reboot this node now? The agent session will drop. KeyStone units come back if they are enabled.";
        if (!window.confirm(msg)) ev.preventDefault();
      });
      const rebootBtn = document.createElement("button");
      rebootBtn.type = "submit";
      rebootBtn.className = "danger";
      rebootBtn.textContent = "Reboot node";
      rebootForm.appendChild(rebootBtn);
      actions.appendChild(rebootForm);
    }

    const ifaces = Array.isArray(data.interfaces) ? data.interfaces : [];
    const table = document.createElement("table");
    table.appendChild(thead(["Interface", "IPv4", "IPv6", "State"]));
    const body = document.createElement("tbody");
    if (!ifaces.length) {
      body.appendChild(emptyRow(4, "No addresses (or ip is missing)."));
    } else {
      ifaces.forEach((iface) => {
        const tr = document.createElement("tr");
        tr.appendChild(tdCode(iface.name || ""));
        const addrs = Array.isArray(iface.ipv4) ? iface.ipv4.join(", ") : "";
        tr.appendChild(tdText(addrs || "—"));
        const v6 = Array.isArray(iface.ipv6) ? iface.ipv6.join(", ") : "";
        tr.appendChild(tdText(v6 || "—"));
        const td = document.createElement("td");
        td.appendChild(el("span", iface.up ? "chip tone-ok" : "chip", iface.up ? "up" : "down"));
        tr.appendChild(td);
        body.appendChild(tr);
      });
    }
    table.appendChild(body);
    health.appendChild(el("h3", null, "Addresses"));
    health.appendChild(table);

    const listed = Array.isArray(data.packages);
    const pkgs = listed ? data.packages : [];
    const pkgHead = el("div", "compose-head");
    pkgHead.appendChild(el("h3", null, "Pending apt upgrades"));
    if (helperOn) {
      const tools = el("div", "actions");
      const check = document.createElement("button");
      check.type = "button";
      check.textContent = "Check for updates";
      check.addEventListener("click", async () => {
        check.disabled = true;
        check.textContent = "Checking…";
        try {
          const r = await fetch("/api/v1/nodes/" + encodeURIComponent(node) + "/sys/updates");
          const body = await r.json().catch(() => ({}));
          if (!r.ok) {
            window.alert(body.error || "Could not list updates.");
            return;
          }
          data.packages = body.packages || [];
          data.capped = !!body.capped;
          host.setAttribute("data-json", JSON.stringify(Object.assign({}, data, { packages: data.packages, capped: data.capped })));
          paintSystem(host);
        } catch (e) {
          window.alert("Could not list updates.");
        } finally {
          check.disabled = false;
          check.textContent = "Check for updates";
        }
      });
      tools.appendChild(check);
      if (manage) {
        const apply = document.createElement("button");
        apply.type = "button";
        apply.className = "danger";
        apply.textContent = "Apply updates";
        apply.addEventListener("click", () => {
          if (!window.confirm("Apply pending apt upgrades on this node? Ubuntu will not auto-restart docker or ssh during Apply; leftover services are listed after.")) {
            return;
          }
          window.location.href = "/nodes/" + encodeURIComponent(node) + "/sys/updates";
        });
        tools.appendChild(apply);
        const autoremove = document.createElement("button");
        autoremove.type = "button";
        autoremove.textContent = "Autoremove";
        autoremove.addEventListener("click", () => {
          if (!window.confirm("Remove unused packages with apt-get autoremove on this node? This does not dist-upgrade.")) {
            return;
          }
          window.location.href = "/nodes/" + encodeURIComponent(node) + "/sys/autoremove";
        });
        tools.appendChild(autoremove);
      }
      pkgHead.appendChild(tools);
    }
    actions.appendChild(pkgHead);
    if (!pkgs.length) {
      actions.appendChild(el("p", "muted", listed ? "No pending upgrades." : "No list yet. Check for updates runs apt-get update on the node."));
    } else {
      if (data.capped) {
        actions.appendChild(el("p", "muted", "Showing the first 500 packages."));
      }
      const pt = document.createElement("table");
      pt.appendChild(thead(["Package", "From", "To"]));
      const pb = document.createElement("tbody");
      pkgs.forEach((p) => {
        const tr = document.createElement("tr");
        tr.appendChild(tdCode(p.name || ""));
        tr.appendChild(tdText(p.from || ""));
        tr.appendChild(tdText(p.to || ""));
        pb.appendChild(tr);
      });
      pt.appendChild(pb);
      actions.appendChild(pt);
    }

    const gitlab = data.gitlab || {};
    if (gitlab.kind === "omnibus") {
      health.appendChild(el("h3", null, "GitLab dump"));
      if (helperOn) {
        if (typeof gitlab.backup_unix === "number") {
          health.appendChild(el("p", "muted", "Last dump " + relativeUnix(gitlab.backup_unix) + (gitlab.backup_name ? " (" + gitlab.backup_name + ")" : "") + "."));
        } else {
          health.appendChild(el("p", "muted", "No dump on disk."));
        }
      } else {
        health.appendChild(el("p", "muted", "Omnibus GitLab is on this node."));
      }
      const glHead = el("div", "compose-head");
      glHead.appendChild(el("h3", null, "GitLab"));
      actions.appendChild(glHead);
      actions.appendChild(el("p", "muted", "Omnibus gitlab-backup on this node (not Docker GitLab). Create writes *_gitlab_backup.tar under /var/opt/gitlab/backups. Restore picks a listed dump (not a path textbox). Copy /etc/gitlab (gitlab.rb and gitlab-secrets.json) yourself — they are not in the tar."));
      if (stepUpErr === "step-up") {
        actions.appendChild(el("p", "error", "This change needs a current authenticator code, not a backup code."));
      } else if (stepUpErr === "step-up-locked") {
        actions.appendChild(el("p", "error", "too many attempts; try again in a few minutes"));
      }
      if (helperOn && manage) {
        const backup = document.createElement("button");
        backup.type = "button";
        backup.textContent = "Backup GitLab";
        backup.addEventListener("click", () => {
          if (!window.confirm("Create a GitLab Omnibus backup on this node? This can take a while and load the disk.")) {
            return;
          }
          window.location.href = "/nodes/" + encodeURIComponent(node) + "/sys/gitlab-backup";
        });
        actions.appendChild(backup);
        const dumps = Array.isArray(gitlab.backups) ? gitlab.backups : [];
        if (dumps.length) {
          if (totpOn) {
            const totp = document.createElement("input");
            totp.id = "sys-gitlab-restore-totp";
            totp.inputMode = "numeric";
            totp.autocomplete = "one-time-code";
            totp.maxLength = 6;
            totp.required = true;
            totp.placeholder = "000000";
            const totpLab = labeled("Authenticator code", totp);
            totpLab.appendChild(el("span", "muted", " Current 6-digit code to restore. Backup codes are for sign-in only."));
            actions.appendChild(totpLab);
          }
          const dt = document.createElement("table");
          dt.appendChild(thead(["Dump", ""]));
          const db = document.createElement("tbody");
          dumps.forEach((row) => {
            const dumpName = (row && row.name) || "";
            if (!dumpName) return;
            const tr = document.createElement("tr");
            tr.appendChild(tdCode(dumpName));
            const td = document.createElement("td");
            td.appendChild(gitlabRestoreForm(node, dumpName, totpOn));
            tr.appendChild(td);
            db.appendChild(tr);
          });
          dt.appendChild(db);
          actions.appendChild(dt);
        } else {
          actions.appendChild(el("p", "muted", "No dump on disk to restore."));
        }
      } else if (!helperOn) {
        actions.appendChild(el("p", "muted", "Enable the system helper to run gitlab-backup."));
      }
    }

    if (manage && helperOn) {
      const netHead = el("div", "compose-head");
      netHead.appendChild(el("h3", null, "Addressing"));
      actions.appendChild(netHead);
      actions.appendChild(el("p", "muted", "Setting a static address can drop this agent session. Keep a console. Ethernet only this version. Add a VLAN below, then Apply addressing on it. Wi-Fi join is a separate form."));
      if (stepUpErr === "step-up") {
        actions.appendChild(el("p", "error", "This change needs a current authenticator code, not a backup code."));
      } else if (stepUpErr === "step-up-locked") {
        actions.appendChild(el("p", "error", "too many attempts; try again in a few minutes"));
      }
      const net = data.net || {};
      const ether = ifaces.filter((i) => ethernetIface(i.name || ""));
      if (!ether.length) {
        actions.appendChild(el("p", "muted", "No Ethernet interface to edit (Wi-Fi, docker, and virtual NICs are skipped)."));
      } else {
        const form = document.createElement("form");
        form.method = "post";
        form.action = "/nodes/" + encodeURIComponent(node) + "/sys/net_set";
        form.className = "sys-net";
        form.addEventListener("submit", (ev) => {
          if (!window.confirm("Change addressing on this node? IPv4 or IPv6 can drop the agent until you reconnect.")) {
            ev.preventDefault();
            return;
          }
          if (totpOn) {
            const field = form.querySelector('input[name="totp"]');
            const digits = ((field && field.value) || "").replace(/\D/g, "");
            if (digits.length !== 6) {
              ev.preventDefault();
            }
          }
        });
        const iface = document.createElement("select");
        iface.name = "iface";
        ether.forEach((i) => {
          const o = document.createElement("option");
          o.value = i.name || "";
          o.textContent = i.name || "";
          if ((net.iface || "") === i.name) o.selected = true;
          iface.appendChild(o);
        });
        const method = document.createElement("select");
        method.name = "method";
        ["dhcp", "static"].forEach((m) => {
          const o = document.createElement("option");
          o.value = m;
          o.textContent = m === "dhcp" ? "DHCP" : "Static";
          if ((net.method || "dhcp") === m) o.selected = true;
          method.appendChild(o);
        });
        function labeled(name, input, span) {
          const lab = document.createElement("label");
          if (span) lab.className = span;
          lab.appendChild(document.createTextNode(name));
          lab.appendChild(input);
          return lab;
        }
        form.appendChild(labeled("Interface", iface));
        form.appendChild(labeled("IPv4", method));
        const address = document.createElement("input");
        address.name = "address";
        address.placeholder = "192.168.0.50";
        address.value = net.address || "";
        address.autocomplete = "off";
        const prefix = document.createElement("input");
        prefix.name = "prefix";
        prefix.type = "number";
        prefix.min = "1";
        prefix.max = "32";
        prefix.value = net.prefix ? String(net.prefix) : "24";
        const gateway = document.createElement("input");
        gateway.name = "gateway";
        gateway.placeholder = "192.168.0.1";
        gateway.value = net.gateway || "";
        gateway.autocomplete = "off";
        const dns = document.createElement("input");
        dns.name = "dns";
        dns.placeholder = "1.1.1.1";
        dns.value = Array.isArray(net.dns) ? net.dns.join(" ") : "";
        dns.autocomplete = "off";
        const addrLab = labeled("Address", address);
        const prefixLab = labeled("Prefix", prefix);
        const gwLab = labeled("Gateway", gateway);
        const dnsLab = labeled("DNS", dns);
        form.appendChild(addrLab);
        form.appendChild(prefixLab);
        form.appendChild(gwLab);
        form.appendChild(dnsLab);
        function setStaticVisible() {
          const on = method.value === "static";
          [addrLab, prefixLab, gwLab, dnsLab].forEach((lab) => {
            lab.hidden = !on;
          });
          [address, prefix, gateway, dns].forEach((inp) => {
            inp.disabled = !on;
          });
        }
        method.addEventListener("change", setStaticVisible);
        setStaticVisible();
        const ipv6Method = document.createElement("select");
        ipv6Method.name = "ipv6_method";
        [["auto", "Automatic (SLAAC/DHCPv6)"], ["static", "Static"]].forEach((pair) => {
          const o = document.createElement("option");
          o.value = pair[0];
          o.textContent = pair[1];
          if ((net.ipv6_method || "auto") === pair[0]) o.selected = true;
          ipv6Method.appendChild(o);
        });
        form.appendChild(labeled("IPv6", ipv6Method, "span-2"));
        const ipv6Address = document.createElement("input");
        ipv6Address.name = "ipv6_address";
        ipv6Address.placeholder = "2001:db8::10";
        ipv6Address.value = net.ipv6_address || "";
        ipv6Address.autocomplete = "off";
        const ipv6Prefix = document.createElement("input");
        ipv6Prefix.name = "ipv6_prefix";
        ipv6Prefix.type = "number";
        ipv6Prefix.min = "1";
        ipv6Prefix.max = "128";
        ipv6Prefix.value = net.ipv6_prefix ? String(net.ipv6_prefix) : "64";
        const ipv6Gateway = document.createElement("input");
        ipv6Gateway.name = "ipv6_gateway";
        ipv6Gateway.placeholder = "2001:db8::1";
        ipv6Gateway.value = net.ipv6_gateway || "";
        ipv6Gateway.autocomplete = "off";
        const ipv6Dns = document.createElement("input");
        ipv6Dns.name = "ipv6_dns";
        ipv6Dns.placeholder = "2001:db8::53";
        ipv6Dns.value = Array.isArray(net.ipv6_dns) ? net.ipv6_dns.join(" ") : "";
        ipv6Dns.autocomplete = "off";
        const ipv6AddrLab = labeled("IPv6 address", ipv6Address);
        const ipv6PrefixLab = labeled("Prefix", ipv6Prefix);
        const ipv6GwLab = labeled("Gateway", ipv6Gateway);
        const ipv6DnsLab = labeled("DNS", ipv6Dns);
        form.appendChild(ipv6AddrLab);
        form.appendChild(ipv6PrefixLab);
        form.appendChild(ipv6GwLab);
        form.appendChild(ipv6DnsLab);
        function setIpv6StaticVisible() {
          const on = ipv6Method.value === "static";
          [ipv6AddrLab, ipv6PrefixLab, ipv6GwLab, ipv6DnsLab].forEach((lab) => {
            lab.hidden = !on;
          });
          [ipv6Address, ipv6Prefix, ipv6Gateway, ipv6Dns].forEach((inp) => {
            inp.disabled = !on;
          });
        }
        ipv6Method.addEventListener("change", setIpv6StaticVisible);
        setIpv6StaticVisible();
        if (totpOn) {
          const totp = document.createElement("input");
          totp.name = "totp";
          totp.inputMode = "numeric";
          totp.autocomplete = "one-time-code";
          totp.maxLength = 6;
          totp.required = true;
          totp.placeholder = "000000";
          const totpLab = labeled("Authenticator code", totp, "span-2");
          totpLab.appendChild(el("span", "muted", "Current 6-digit code. Backup codes are for sign-in only."));
          form.appendChild(totpLab);
        }
        const save = document.createElement("button");
        save.type = "submit";
        save.className = "danger span-2";
        save.textContent = "Apply addressing";
        form.appendChild(save);
        actions.appendChild(form);
      }
      const parents = ether.filter((i) => String(i.name || "").indexOf(".") < 0);
      const vlanHead = el("div", "compose-head");
      vlanHead.appendChild(el("h3", null, "VLAN"));
      actions.appendChild(vlanHead);
      actions.appendChild(el("p", "muted", "Creates parent.id (for example eth0.10). Not a name textbox. QinQ, delete, and hotspot/802.1X are not in this UI. Applying can bounce the NIC — keep a console."));
      if (!parents.length) {
        actions.appendChild(el("p", "muted", "No Ethernet parent to add a VLAN on."));
      } else {
        const vform = document.createElement("form");
        vform.method = "post";
        vform.action = "/nodes/" + encodeURIComponent(node) + "/sys/vlan_add";
        vform.className = "sys-net";
        vform.addEventListener("submit", (ev) => {
          const parent = vform.querySelector('select[name="iface"]');
          const idField = vform.querySelector('input[name="vlan"]');
          const parentName = (parent && parent.value) || "";
          const vid = ((idField && idField.value) || "").replace(/\D/g, "");
          if (!window.confirm("Create VLAN " + vid + " on " + parentName + " as " + parentName + "." + vid + "? This can drop the agent until you reconnect.")) {
            ev.preventDefault();
            return;
          }
          if (totpOn) {
            const field = vform.querySelector('input[name="totp"]');
            const digits = ((field && field.value) || "").replace(/\D/g, "");
            if (digits.length !== 6) {
              ev.preventDefault();
            }
          }
        });
        const parentSel = document.createElement("select");
        parentSel.name = "iface";
        parents.forEach((i) => {
          const o = document.createElement("option");
          o.value = i.name || "";
          o.textContent = i.name || "";
          parentSel.appendChild(o);
        });
        const vlanId = document.createElement("input");
        vlanId.name = "vlan";
        vlanId.type = "number";
        vlanId.min = "1";
        vlanId.max = "4094";
        vlanId.value = "10";
        vlanId.required = true;
        function vlabeled(name, input, span) {
          const lab = document.createElement("label");
          if (span) lab.className = span;
          lab.appendChild(document.createTextNode(name));
          lab.appendChild(input);
          return lab;
        }
        vform.appendChild(vlabeled("Parent", parentSel));
        vform.appendChild(vlabeled("VLAN id", vlanId));
        if (totpOn) {
          const totp = document.createElement("input");
          totp.name = "totp";
          totp.inputMode = "numeric";
          totp.autocomplete = "one-time-code";
          totp.maxLength = 6;
          totp.required = true;
          totp.placeholder = "000000";
          const totpLab = vlabeled("Authenticator code", totp, "span-2");
          totpLab.appendChild(el("span", "muted", "Current 6-digit code. Backup codes are for sign-in only."));
          vform.appendChild(totpLab);
        }
        const add = document.createElement("button");
        add.type = "submit";
        add.className = "danger span-2";
        add.textContent = "Add VLAN";
        vform.appendChild(add);
        actions.appendChild(vform);
      }
      const radios = ifaces.filter((i) => wifiIface(i.name || ""));
      const wifiHead = el("div", "compose-head");
      wifiHead.appendChild(el("h3", null, "Wi-Fi"));
      actions.appendChild(wifiHead);
      actions.appendChild(el("p", "muted", "Scan, then join a listed SSID (not an SSID textbox). DHCP/SLAAC only. Hidden networks, hotspot, and 802.1X are not in this UI. Joining can drop the agent — keep a console."));
      if (!radios.length) {
        actions.appendChild(el("p", "muted", "No wireless interface on this node."));
      } else {
        const wform = document.createElement("form");
        wform.method = "post";
        wform.action = "/nodes/" + encodeURIComponent(node) + "/sys/wifi_join";
        wform.className = "sys-net";
        wform.addEventListener("submit", (ev) => {
          const radio = wform.querySelector('select[name="iface"]');
          const ssidSel = wform.querySelector('select[name="ssid"]');
          const ifn = (radio && radio.value) || "";
          const ssid = (ssidSel && ssidSel.value) || "";
          if (!ssid) {
            ev.preventDefault();
            return;
          }
          if (!window.confirm("Join Wi-Fi " + ssid + " on " + ifn + "? This can drop the agent until you reconnect.")) {
            ev.preventDefault();
            return;
          }
          if (totpOn) {
            const field = wform.querySelector('input[name="totp"]');
            const digits = ((field && field.value) || "").replace(/\D/g, "");
            if (digits.length !== 6) {
              ev.preventDefault();
            }
          }
        });
        const radioSel = document.createElement("select");
        radioSel.name = "iface";
        radios.forEach((i) => {
          const o = document.createElement("option");
          o.value = i.name || "";
          o.textContent = i.name || "";
          radioSel.appendChild(o);
        });
        const ssidSel = document.createElement("select");
        ssidSel.name = "ssid";
        const empty = document.createElement("option");
        empty.value = "";
        empty.textContent = "Scan first";
        ssidSel.appendChild(empty);
        const psk = document.createElement("input");
        psk.name = "psk";
        psk.type = "password";
        psk.autocomplete = "off";
        psk.required = true;
        psk.minLength = 8;
        function wlabeled(name, input, span) {
          const lab = document.createElement("label");
          if (span) lab.className = span;
          lab.appendChild(document.createTextNode(name));
          lab.appendChild(input);
          return lab;
        }
        wform.appendChild(wlabeled("Interface", radioSel));
        const scanBtn = document.createElement("button");
        scanBtn.type = "button";
        scanBtn.textContent = "Scan";
        scanBtn.addEventListener("click", async () => {
          scanBtn.disabled = true;
          scanBtn.textContent = "Scanning…";
          try {
            const r = await fetch("/api/v1/nodes/" + encodeURIComponent(node) + "/sys/wifi?iface=" + encodeURIComponent(radioSel.value));
            const data = await r.json();
            const ssids = Array.isArray(data.ssids) ? data.ssids : [];
            ssidSel.replaceChildren();
            if (!ssids.length) {
              const o = document.createElement("option");
              o.value = "";
              o.textContent = "No SSID in range";
              ssidSel.appendChild(o);
            } else {
              ssids.forEach((s) => {
                const o = document.createElement("option");
                o.value = s;
                o.textContent = s;
                ssidSel.appendChild(o);
              });
            }
          } catch (e) {
            ssidSel.replaceChildren();
            const o = document.createElement("option");
            o.value = "";
            o.textContent = "Scan failed";
            ssidSel.appendChild(o);
          }
          scanBtn.disabled = false;
          scanBtn.textContent = "Scan";
        });
        wform.appendChild(scanBtn);
        wform.appendChild(wlabeled("SSID", ssidSel, "span-2"));
        wform.appendChild(wlabeled("Password", psk, "span-2"));
        if (totpOn) {
          const totp = document.createElement("input");
          totp.name = "totp";
          totp.inputMode = "numeric";
          totp.autocomplete = "one-time-code";
          totp.maxLength = 6;
          totp.required = true;
          totp.placeholder = "000000";
          const totpLab = wlabeled("Authenticator code", totp, "span-2");
          totpLab.appendChild(el("span", "muted", "Current 6-digit code. Backup codes are for sign-in only."));
          wform.appendChild(totpLab);
        }
        const join = document.createElement("button");
        join.type = "submit";
        join.className = "danger span-2";
        join.textContent = "Join Wi-Fi";
        wform.appendChild(join);
        actions.appendChild(wform);
      }
      const sshHead = el("div", "compose-head");
      sshHead.appendChild(el("h3", null, "SSH password"));
      actions.appendChild(sshHead);
      actions.appendChild(el("p", "muted", "Allow password logins or keys only (not a user editor). Writes /etc/ssh/sshd_config.d/00-keystone.conf then reloads ssh. Keep keys or a console — turning passwords off can lock you out."));
      if (!(data.ssh && data.ssh.available)) {
        actions.appendChild(el("p", "muted", "sshd is not available on this node."));
      } else {
        const sform = document.createElement("form");
        sform.method = "post";
        sform.action = "/nodes/" + encodeURIComponent(node) + "/sys/ssh_password";
        sform.className = "sys-net";
        sform.addEventListener("submit", (ev) => {
          const sel = sform.querySelector('select[name="password_auth"]');
          const want = (sel && sel.value) || "";
          const msg = want === "no"
            ? "Refuse SSH password logins on this host? You will need keys or a console. This can lock you out."
            : "Allow SSH password logins on this host?";
          if (!window.confirm(msg)) {
            ev.preventDefault();
            return;
          }
          if (totpOn) {
            const field = sform.querySelector('input[name="totp"]');
            const digits = ((field && field.value) || "").replace(/\D/g, "");
            if (digits.length !== 6) {
              ev.preventDefault();
            }
          }
        });
        const authSel = document.createElement("select");
        authSel.name = "password_auth";
        [["yes", "Allow password logins"], ["no", "Keys only"]].forEach((pair) => {
          const o = document.createElement("option");
          o.value = pair[0];
          o.textContent = pair[1];
          authSel.appendChild(o);
        });
        authSel.value = data.ssh.password_auth ? "yes" : "no";
        function slabeled(name, input, span) {
          const lab = document.createElement("label");
          if (span) lab.className = span;
          lab.appendChild(document.createTextNode(name));
          lab.appendChild(input);
          return lab;
        }
        sform.appendChild(slabeled("Password logins", authSel, "span-2"));
        if (totpOn) {
          const totp = document.createElement("input");
          totp.name = "totp";
          totp.inputMode = "numeric";
          totp.autocomplete = "one-time-code";
          totp.maxLength = 6;
          totp.required = true;
          totp.placeholder = "000000";
          const totpLab = slabeled("Authenticator code", totp, "span-2");
          totpLab.appendChild(el("span", "muted", "Current 6-digit code. Backup codes are for sign-in only."));
          sform.appendChild(totpLab);
        }
        const applySsh = document.createElement("button");
        applySsh.type = "submit";
        applySsh.className = "danger span-2";
        applySsh.textContent = "Apply SSH password";
        sform.appendChild(applySsh);
        actions.appendChild(sform);
      }
    }
    split.appendChild(health);
    split.appendChild(actions);
    wrap.appendChild(split);
    host.replaceChildren(wrap);
  }

  function restartUnitForm(node, unit, totpOn, uiHost) {
    const form = document.createElement("form");
    form.method = "post";
    form.action = "/nodes/" + encodeURIComponent(node) + "/sys/unit_restart";
    form.className = "inline";
    const hidden = document.createElement("input");
    hidden.type = "hidden";
    hidden.name = "unit";
    hidden.value = unit;
    form.appendChild(hidden);
    form.addEventListener("submit", (ev) => {
      const sensitive = unit === "keystone-server.service" || unit === "docker.service" || unit === "ssh.service";
      let msg = "Restart " + unit + " with systemctl restart?";
      if (uiHost && unit === "keystone-server.service") {
        msg = "This node is serving the KeyStone UI. Restarting keystone-server.service will take this session down until the server is back. Continue?";
      } else if (sensitive) {
        msg = "Restart " + unit + "? docker or ssh bouncing will drop containers or SSH. Continue?";
      }
      if (!window.confirm(msg)) {
        ev.preventDefault();
        return;
      }
      if (totpOn) {
        const shared = document.getElementById("sys-restart-totp");
        const digits = ((shared && shared.value) || "").replace(/\D/g, "");
        if (digits.length !== 6) {
          ev.preventDefault();
          return;
        }
        const t = document.createElement("input");
        t.type = "hidden";
        t.name = "totp";
        t.value = digits;
        form.appendChild(t);
      }
    });
    const btn = document.createElement("button");
    btn.type = "submit";
    btn.textContent = "Restart";
    form.appendChild(btn);
    return form;
  }

  function gitlabRestoreForm(node, name, totpOn) {
    const form = document.createElement("form");
    form.method = "post";
    form.action = "/nodes/" + encodeURIComponent(node) + "/sys/gitlab_restore";
    form.className = "inline";
    const hidden = document.createElement("input");
    hidden.type = "hidden";
    hidden.name = "name";
    hidden.value = name;
    form.appendChild(hidden);
    form.addEventListener("submit", (ev) => {
      if (!window.confirm("Restore GitLab on this node from " + name + "? This replaces GitLab data. gitlab-ctl will stop puma and sidekiq, then restart. /etc/gitlab is not in the archive.")) {
        ev.preventDefault();
        return;
      }
      if (totpOn) {
        const shared = document.getElementById("sys-gitlab-restore-totp");
        const digits = ((shared && shared.value) || "").replace(/\D/g, "");
        if (digits.length !== 6) {
          ev.preventDefault();
          return;
        }
        const t = document.createElement("input");
        t.type = "hidden";
        t.name = "totp";
        t.value = digits;
        form.appendChild(t);
      }
    });
    const btn = document.createElement("button");
    btn.type = "submit";
    btn.className = "danger";
    btn.textContent = "Restore";
    form.appendChild(btn);
    return form;
  }

  function unitNameTable(names, opts) {
    const manage = opts && opts.manage;
    const table = document.createElement("table");
    table.appendChild(thead(manage ? ["Unit", ""] : ["Unit"]));
    const body = document.createElement("tbody");
    names.forEach((name) => {
      const tr = document.createElement("tr");
      tr.appendChild(tdCode(name || ""));
      if (manage) {
        const td = document.createElement("td");
        td.appendChild(restartUnitForm(opts.node, name, opts.totpOn, opts.uiHost));
        tr.appendChild(td);
      }
      body.appendChild(tr);
    });
    table.appendChild(body);
    return table;
  }

  function relativeUnix(unix) {
    const n = Number(unix);
    if (!Number.isFinite(n) || n <= 0) return "unknown";
    const secs = Math.max(0, Math.floor(Date.now() / 1000 - n));
    if (secs < 60) return "just now";
    if (secs < 3600) return Math.floor(secs / 60) + "m ago";
    if (secs < 86400) return Math.floor(secs / 3600) + "h ago";
    return Math.floor(secs / 86400) + "d ago";
  }

  function ethernetIface(name) {
    const n = String(name || "").toLowerCase();
    if (!n || n === "lo" || n.indexOf("lo.") === 0) return false;
    if (n.indexOf("wl") === 0 || n.indexOf("ww") === 0) return false;
    if (n.indexOf("docker") === 0 || n.indexOf("br-") === 0 || n.indexOf("veth") === 0) return false;
    if (n.indexOf("virbr") === 0 || n.indexOf("cni") === 0 || n.indexOf("flannel") === 0) return false;
    if (n.indexOf("tun") === 0 || n.indexOf("tap") === 0) return false;
    return true;
  }

  function wifiIface(name) {
    const n = String(name || "").toLowerCase();
    return n.indexOf("wl") === 0;
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

  function sparkline(points, area) {
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
    const coords = points.map((p, i) => {
      const x = pad + i * (240 - 2 * pad) / Math.max(points.length - 1, 1);
      const y = 64 - pad - ((p.v - min) / span) * (64 - 2 * pad);
      return [x, y];
    });
    const d = coords.map((xy, i) => {
      return (i ? "L" : "M") + xy[0].toFixed(1) + " " + xy[1].toFixed(1);
    }).join(" ");
    if (area) {
      const first = coords[0];
      const last = coords[coords.length - 1];
      const fill = document.createElementNS("http://www.w3.org/2000/svg", "path");
      fill.setAttribute(
        "d",
        d + " L" + last[0].toFixed(1) + " " + (64 - pad) + " L" + first[0].toFixed(1) + " " + (64 - pad) + " Z"
      );
      fill.setAttribute("class", "spark-fill");
      svg.appendChild(fill);
    }
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    svg.appendChild(path);
    return svg;
  }

  function widgetStyle(kind, style) {
    const s = String(style || "");
    if (kind === "gauge") return s === "bar" ? "bar" : "donut";
    if (kind === "sparkline") return s === "area" ? "area" : "line";
    if (kind === "stat") return s === "compact" ? "compact" : "large";
    if (kind === "bar_list") return s === "compact" ? "compact" : "bars";
    return s || "default";
  }

  function styleChoices(kind) {
    if (kind === "gauge") return [["donut", "Donut"], ["bar", "Bar"]];
    if (kind === "sparkline") return [["line", "Line"], ["area", "Area"]];
    if (kind === "stat") return [["large", "Large"], ["compact", "Compact"]];
    if (kind === "bar_list") return [["bars", "Bars"], ["compact", "Compact"]];
    return [];
  }

  function widgetIsEmpty(w) {
    if (w.kind === "gauge") return w.ratio == null;
    if (w.kind === "bar_list") return !(w.rows && w.rows.length);
    if (w.kind === "sparkline") return w.display === "—" && !(w.spark && w.spark.length);
    return w.display === "—";
  }

  function renderWidget(w) {
    const style = widgetStyle(w.kind, w.style);
    const card = el("article", "widget span-" + (w.span || 1) + " style-" + style + (w.tone ? " tone-" + w.tone : ""));
    card.appendChild(el("h3", null, w.title || w.id));
    if (widgetIsEmpty(w)) card.classList.add("empty");
    if (w.kind === "gauge") {
      if (w.ratio == null) {
        card.appendChild(el("div", "stat", "—"));
      } else if (style === "bar") {
        const wrap = el("div", "gauge-bar-wrap");
        wrap.appendChild(el("div", "stat", w.display || "—"));
        const track = el("div", "bar-track gauge-bar");
        const fill = el("div", "bar-fill");
        fill.style.width = Math.round((Number(w.ratio) || 0) * 100) + "%";
        track.appendChild(fill);
        wrap.appendChild(track);
        card.appendChild(wrap);
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
      card.appendChild(sparkline(w.spark || [], style === "area"));
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

    function clampChoice(value, allowed, fallback) {
      return allowed.indexOf(value) >= 0 ? value : fallback;
    }

    function pageStyle() {
      const p = (layout && layout.page) || {};
      return {
        density: clampChoice(p.density, ["compact", "comfortable", "spacious"], "comfortable"),
        cards: clampChoice(p.cards, ["bordered", "flush", "raised"], "bordered"),
        accent: clampChoice(p.accent, ["blue", "green", "amber", "rose"], "blue"),
        empty: clampChoice(p.empty, ["show", "hide"], "show")
      };
    }

    function setPage(field, value) {
      layout.page = Object.assign(pageStyle(), { [field]: value });
      paintAll();
    }

    function setWidgetStyle(idx, value) {
      const kind = layout.widgets[idx].kind;
      const choices = styleChoices(kind).map((pair) => pair[0]);
      const fallback = choices[0] || "";
      layout.widgets[idx].style = clampChoice(value, choices, fallback);
      paintAll();
    }

    function paintWidgets() {
      const byId = {};
      (Array.isArray(hydrated) ? hydrated : []).forEach((w) => {
        byId[w.id] = w;
      });
      const page = pageStyle();
      const grid = el("div", "widget-grid density-" + page.density + " cards-" + page.cards + " accent-" + page.accent);
      const items = (layout && layout.widgets) || [];
      if (!items.length) {
        grid.appendChild(el("p", "muted", "No widgets yet. Use Customize to add some."));
      }
      let shown = 0;
      items.forEach((spec, idx) => {
        const h = Object.assign({
          id: spec.id,
          kind: spec.kind,
          title: spec.title,
          span: spec.span,
          style: spec.style || "",
          display: "—",
          tone: "",
          rows: [],
          spark: []
        }, byId[spec.id] || {});
        h.title = spec.title;
        h.span = spec.span;
        h.kind = spec.kind;
        h.style = spec.style || h.style || "";
        if (!editing && page.empty === "hide" && widgetIsEmpty(h)) return;
        shown += 1;
        const card = renderWidget(h);
        if (editing) {
          card.classList.add("editing");
          card.insertBefore(editChrome(idx, card), card.firstChild);
          bindDrag(card, idx);
        }
        grid.appendChild(card);
      });
      if (items.length && !shown) {
        grid.appendChild(el("p", "muted", "No widgets with data yet. Empty cards are hidden."));
      }
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
      card.querySelectorAll("button, select, input").forEach((b) => {
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

    function editChrome(idx, card) {
      const row = el("div", "widget-edit");
      const n = layout.widgets.length;
      const span = Number(layout.widgets[idx].span) || 1;
      const grip = el("span", "widget-grip", "⋮⋮");
      grip.title = "Drag to move";
      grip.setAttribute("aria-hidden", "true");
      row.appendChild(grip);
      const titleIn = document.createElement("input");
      titleIn.type = "text";
      titleIn.value = layout.widgets[idx].title || "";
      titleIn.maxLength = 48;
      titleIn.setAttribute("aria-label", "Card title");
      titleIn.addEventListener("input", () => {
        const v = titleIn.value.trim();
        layout.widgets[idx].title = v || layout.widgets[idx].id;
        const h3 = card.querySelector("h3");
        if (h3) h3.textContent = layout.widgets[idx].title;
      });
      titleIn.addEventListener("change", () => {
        if (!titleIn.value.trim()) titleIn.value = layout.widgets[idx].title;
      });
      row.appendChild(titleIn);
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
      const styles = styleChoices(layout.widgets[idx].kind);
      if (styles.length) {
        const sel = document.createElement("select");
        sel.setAttribute("aria-label", "Card style");
        const current = widgetStyle(layout.widgets[idx].kind, layout.widgets[idx].style);
        styles.forEach((pair) => {
          const o = new Option(pair[1], pair[0]);
          if (pair[0] === current) o.selected = true;
          sel.appendChild(o);
        });
        sel.addEventListener("change", () => setWidgetStyle(idx, sel.value));
        sel.addEventListener("mousedown", (ev) => ev.stopPropagation());
        sel.addEventListener("dragstart", (ev) => ev.preventDefault());
        row.appendChild(sel);
      }
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
      const page = pageStyle();
      function addPageSelect(field, label, options, current) {
        const sel = document.createElement("select");
        sel.setAttribute("aria-label", label);
        options.forEach((pair) => {
          const o = new Option(pair[1], pair[0]);
          if (pair[0] === current) o.selected = true;
          sel.appendChild(o);
        });
        sel.addEventListener("change", () => setPage(field, sel.value));
        toolbar.appendChild(sel);
      }
      addPageSelect("density", "Density", [["compact", "Compact"], ["comfortable", "Comfortable"], ["spacious", "Spacious"]], page.density);
      addPageSelect("cards", "Cards", [["bordered", "Bordered"], ["flush", "Flush"], ["raised", "Raised"]], page.cards);
      addPageSelect("accent", "Accent", [["blue", "Blue"], ["green", "Green"], ["amber", "Amber"], ["rose", "Rose"]], page.accent);
      addPageSelect("empty", "Empty cards", [["show", "Show empty"], ["hide", "Hide empty"]], page.empty);
      toolbar.appendChild(el("span", "muted", "Drag cards to place them. +/− changes width. Rename and style are per card."));
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
        sel: "nav a[href='/audit']",
        title: "Audit",
        body: "Start, stop, Compose Update, apt apply, and IPv4 changes from this UI. The ingest token cannot write here."
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

  function bindSessionLifetime() {
    if (!document.querySelector("header.bar")) return;
    // Do not sendBeacon /logout on pagehide. Tab switch, mobile background,
    // and Chrome discard all fire it, which signed people out after ~10 min.
    function beat() {
      fetch("/api/v1/session", { credentials: "same-origin" }).catch(() => {});
    }
    beat();
    setInterval(beat, 30000);
    document.addEventListener("visibilitychange", () => {
      if (!document.hidden) beat();
    });
  }

  bindSessionLifetime();
  if (document.querySelector("header.bar") && wantTour()) {
    setTimeout(startTour, 80);
  }
})();
