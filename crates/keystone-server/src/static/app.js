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
})();
