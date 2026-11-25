/* Terra Studio client. Original work; no frameworks, no network assets. */
(function () {
  "use strict";

  const params = new URLSearchParams(location.search);
  const token = params.get("t") || "";

  const $ = (id) => document.getElementById(id);

  const state = {
    world: null,
    meta: null,
    layer: "atlas",
    year: 10000,
    zoom: 1,
    ox: 0,
    oy: 0,
    drag: null,
    mapImage: null,
    mapLoading: false,
    job: null,
    civOverlay: false,
    events: [],
    offset: 0,
    total: 0,
    selected: null,
    filters: { kind: "all", civ: "all", q: "", from: "", to: "" },
    timeline: null,
    yearTimer: null,
    searchTimer: null,
  };

  function api(path, query) {
    const url = new URL(path, location.origin);
    url.searchParams.set("t", token);
    if (query) {
      for (const [k, v] of Object.entries(query)) {
        if (v !== undefined && v !== null && v !== "") url.searchParams.set(k, v);
      }
    }
    return url.toString();
  }

  async function getJSON(path, query) {
    const res = await fetch(api(path, query));
    const text = await res.text();
    let body = null;
    try { body = JSON.parse(text); } catch (_) { /* non-JSON error */ }
    if (!res.ok) {
      const message = body && body.error ? body.error : "request failed (" + res.status + ")";
      throw new Error(message);
    }
    return body;
  }

  const esc = (s) => String(s == null ? "" : s)
    .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;").replace(/'/g, "&#39;");

  function fmt(n, digits = 0) {
    if (n === null || n === undefined || Number.isNaN(Number(n))) return "—";
    return Number(n).toLocaleString("en-US", { maximumFractionDigits: digits });
  }

  function fmtBig(n) {
    const v = Number(n) || 0;
    if (Math.abs(v) >= 1e9) return (v / 1e9).toFixed(2) + "B";
    if (Math.abs(v) >= 1e6) return (v / 1e6).toFixed(2) + "M";
    if (Math.abs(v) >= 1e3) return (v / 1e3).toFixed(1) + "k";
    return String(Math.round(v));
  }

  function toast(message) {
    const el = $("toast");
    el.textContent = message;
    el.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => { el.hidden = true; }, 5200);
  }

  function setStatus(text, hash) {
    $("status-text").textContent = text;
    if (hash !== undefined) $("status-hash").textContent = hash;
  }

  function civColor(id) {
    const list = state.meta && state.meta.civs ? state.meta.civs : [];
    const civ = list.find((c) => c.id === id);
    return civ && civ.color ? civ.color : "#7cc4ff";
  }

  function civName(id) {
    const list = state.meta && state.meta.civs ? state.meta.civs : [];
    const civ = list.find((c) => c.id === id);
    return civ ? civ.name : "People " + id;
  }

  /* ------------------------------------------------------------- launch */

  $("launch").addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!token) {
      toast("This page needs its Studio token. Reopen the URL printed by the launcher.");
      return;
    }
    const seed = Number($("seed").value) >>> 0;
    const years = Number($("years").value);
    const preset = $("preset").value;
    const civs = Number($("civs").value);
    const body = { seed, years, civs, subdiv: 5 };
    if (preset) body.preset = preset;
    await launchWorld(body);
  });

  async function launchWorld(body) {
    $("generate").disabled = true;
    $("map-empty").hidden = true;
    $("progress").hidden = false;
    $("progress-bar").style.width = "2%";
    $("progress-label").textContent = "queued";
    setStatus("Starting a new planet…");
    try {
      const res = await fetch(api("/api/run"), {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || "could not start generation");
      state.job = data.id;
      watchJob(data.id);
    } catch (err) {
      finishGeneration();
      toast(err.message);
      setStatus("Generation failed to start.");
    }
  }

  let pollTimer = null;
  let stream = null;

  function watchJob(id) {
    if (stream) { stream.close(); stream = null; }
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
    try {
      stream = new EventSource(api("/api/events", { id }));
      stream.addEventListener("progress", (ev) => {
        try { updateProgress(JSON.parse(ev.data)); } catch (_) {}
      });
      stream.addEventListener("complete", () => {
        stream.close();
        stream = null;
        loadWorld(id);
      });
      stream.addEventListener("error", () => {
        if (stream) { stream.close(); stream = null; }
        startPolling(id);
      });
    } catch (_) {
      startPolling(id);
    }
  }

  function startPolling(id) {
    if (pollTimer) return;
    pollTimer = setInterval(async () => {
      try {
        const job = await getJSON("/api/job", { id });
        updateProgress(job);
        if (job.status === "done") {
          clearInterval(pollTimer); pollTimer = null;
          loadWorld(id);
        } else if (job.status === "failed") {
          clearInterval(pollTimer); pollTimer = null;
          finishGeneration();
          toast(job.error || "generation failed");
          setStatus("Generation failed.");
        }
      } catch (err) {
        clearInterval(pollTimer); pollTimer = null;
        finishGeneration();
        toast(err.message);
      }
    }, 400);
  }

  function updateProgress(job) {
    const p = Math.max(0, Math.min(1, Number(job.progress) || 0));
    $("progress-bar").style.width = Math.round(4 + p * 96) + "%";
    $("progress-label").textContent = (job.stage || "working") + " · " + Math.round(p * 100) + "%";
  }

  function finishGeneration() {
    $("generate").disabled = false;
    $("progress").hidden = true;
  }

  /* --------------------------------------------------------------- world */

  async function loadWorld(id) {
    try {
      const meta = await getJSON("/api/world", { id });
      state.world = id;
      state.meta = meta;
      state.year = meta.years;
      state.zoom = 1; state.ox = 0; state.oy = 0;
      state.events = []; state.offset = 0; state.selected = null;

      $("year").min = 0;
      $("year").max = meta.years;
      $("year").step = Math.max(10, Math.round(meta.years / 500));
      $("year").value = meta.years;
      $("year-label").textContent = fmt(meta.years);
      $("timeline-end").textContent = "year " + fmt(meta.years);
      $("timeline-mid").textContent = "year " + fmt(Math.round(meta.years / 2));

      buildLayers(meta.layers);
      buildCivs(meta.civs);
      buildFilters(meta);
      renderStats(meta);
      $("map-empty").hidden = true;
      $("download-map").disabled = false;
      $("export-md").disabled = false;
      $("export-json").disabled = false;
      $("export-csv").disabled = false;
      $("export-world").disabled = false;
      $("cause-body").className = "muted";
      $("cause-body").textContent = "Select an event to trace its causes back to physical facts.";
      setStatus(
        meta.planet + " forged · " + fmt(meta.cells) + " cells · " + fmt(meta.chronicle.events) + " events",
        meta.hash.slice(0, 16)
      );
      finishGeneration();
      await Promise.all([renderMap(), loadChronicle(true), loadTimeline()]);
    } catch (err) {
      finishGeneration();
      toast(err.message);
      setStatus("Could not load the world.");
    }
  }

  function buildLayers(layers) {
    const nav = $("layers");
    nav.innerHTML = "";
    const list = layers && layers.length ? layers : [{ code: "atlas", label: "Atlas", legend: [] }];
    if (!list.some((l) => l.code === state.layer)) state.layer = list[0].code;
    for (const layer of list) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.textContent = layer.label;
      btn.dataset.layer = layer.code;
      btn.className = layer.code === state.layer ? "active" : "";
      btn.addEventListener("click", () => {
        state.layer = layer.code;
        for (const b of nav.querySelectorAll("button")) b.classList.toggle("active", b === btn);
        renderLegend(layer);
        renderMap();
      });
      nav.appendChild(btn);
      if (layer.code === state.layer) renderLegend(layer);
    }
  }

  function renderLegend(layer) {
    const legend = $("legend");
    legend.innerHTML = "";
    if (layer && layer.code === "political" && state.meta) {
      for (const civ of state.meta.civs) {
        const span = document.createElement("span");
        span.className = "chip";
        span.innerHTML = '<span class="swatch" style="background:' + esc(civ.color) + '"></span>' + esc(civ.name);
        legend.appendChild(span);
      }
      return;
    }
    if (!layer || !layer.legend || !layer.legend.length) {
      legend.textContent = "";
      return;
    }
    for (const entry of layer.legend) {
      const span = document.createElement("span");
      span.className = "chip";
      span.innerHTML = '<span class="swatch" style="background:' + esc(entry.color) + '"></span>' + esc(entry.label);
      legend.appendChild(span);
    }
  }

  function renderStats(meta) {
    const s = meta.stats;
    const h = meta.history;
    const c = meta.chronicle;
    const rows = [
      ["planet", meta.planet],
      ["seed", meta.seed],
      ["cells", fmt(meta.cells)],
      ["land", (s.landFraction * 100).toFixed(1) + "%"],
      ["plates", fmt(s.plates)],
      ["rivers", fmt(s.rivers)],
      ["lakes", fmt(s.lakes)],
      ["mean temp", fmt(s.meanTemperatureC, 1) + " °C"],
      ["rain", fmt(s.meanPrecipMm) + " mm"],
      ["species", fmt(s.meanSpecies)],
      ["peoples", fmt(h.surviving) + " / " + fmt(h.civs)],
      ["wars", fmt(h.wars)],
      ["events", fmt(c.events)],
      ["traceable", h.sound ? "yes" : "no"],
    ];
    const dl = $("world-stats");
    dl.innerHTML = "";
    for (const [k, v] of rows) {
      const dt = document.createElement("dt");
      dt.textContent = k;
      const dd = document.createElement("dd");
      dd.textContent = v;
      if (k === "traceable") dd.className = h.sound ? "good" : "bad";
      dl.append(dt, dd);
    }
  }

  function buildCivs(civs) {
    const list = $("civ-list");
    list.innerHTML = "";
    list.classList.remove("muted");
    if (!civs || !civs.length) {
      list.textContent = "No civilizations survived.";
      list.classList.add("muted");
      return;
    }
    for (const civ of civs) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "civ-row" + (civ.alive ? "" : " dead");
      const meta = civ.alive
        ? fmtBig(civ.population) + " · " + fmt(civ.cells) + " cells"
        : "fallen" + (civ.collapsedYear ? " · year " + fmt(civ.collapsedYear) : "");
      row.innerHTML =
        '<span class="swatch" style="background:' + esc(civ.color) + '"></span>' +
        '<span class="name">' + esc(civ.name) + "</span>" +
        '<span class="meta">' + meta + "</span>";
      row.title = civ.alive
        ? civ.name + ", " + fmtBig(civ.population) + " people, technology " + civ.tech
        : civ.name + " collapsed";
      row.addEventListener("click", () => {
        state.filters.civ = String(civ.id);
        $("filter-civ").value = String(civ.id);
        loadChronicle(true);
        $("event-list").scrollIntoView({ behavior: "smooth", block: "start" });
      });
      list.appendChild(row);
    }
  }

  function buildFilters(meta) {
    const kind = $("filter-kind");
    kind.innerHTML = '<option value="all">All events</option>';
    const counts = meta.chronicle.counts || {};
    for (const key of Object.keys(counts).sort()) {
      const opt = document.createElement("option");
      opt.value = key;
      opt.textContent = labelForKind(key) + " (" + counts[key] + ")";
      kind.appendChild(opt);
    }
    const civ = $("filter-civ");
    civ.innerHTML = '<option value="all">Everyone</option>';
    for (const c of meta.civs) {
      const opt = document.createElement("option");
      opt.value = String(c.id);
      opt.textContent = c.name;
      civ.appendChild(opt);
    }
    $("filter-from").max = meta.years;
    $("filter-to").max = meta.years;
    $("filter-to").value = meta.years;
    state.filters = { kind: "all", civ: "all", q: "", from: "", to: "" };
    kind.value = "all"; civ.value = "all";
    $("filter-q").value = "";
    $("filter-from").value = "";
  }

  const KIND_LABELS = {
    world_formed: "World formed", plate_shift: "Plate shift", eruption: "Eruption",
    drought: "Drought", sea_level_change: "Sea level", settlement_founded: "Settlement",
    expansion: "Expansion", resource_discovered: "Resource", trade_route_opened: "Trade",
    war_declared: "War", battle: "Battle", city_captured: "City captured",
    peace_treaty: "Peace", tech_discovered: "Technology", famine: "Famine",
    epidemic: "Epidemic", collapse: "Collapse", migration: "Migration",
    golden_age: "Golden age", extinction: "Extinction",
  };

  function labelForKind(kind) {
    return KIND_LABELS[kind] || kind.replace(/_/g, " ");
  }

  /* ------------------------------------------------------------------ map */

  function mapQuery(width, height) {
    const q = {
      id: state.world,
      layer: state.layer,
      w: width,
      h: height,
      year: state.layer === "political" || state.civOverlay ? state.year : undefined,
    };
    if (state.civOverlay) q.civs = 1;
    return q;
  }

  async function renderMap() {
    if (!state.world) return;
    if (state.mapLoading) return;
    state.mapLoading = true;
    $("map-loading").hidden = false;
    const img = new Image();
    img.onload = () => {
      state.mapImage = img;
      state.mapLoading = false;
      $("map-loading").hidden = true;
      drawMap();
    };
    img.onerror = () => {
      state.mapLoading = false;
      $("map-loading").hidden = true;
      toast("Could not render that layer.");
    };
    img.src = api("/api/map", mapQuery(1024, 512));
  }

  function drawMap() {
    const canvas = $("map");
    const ctx = canvas.getContext("2d");
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#05080b";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    if (!state.mapImage) return;
    ctx.imageSmoothingEnabled = true;
    ctx.setTransform(state.zoom, 0, 0, state.zoom, state.ox, state.oy);
    ctx.drawImage(state.mapImage, 0, 0, canvas.width, canvas.height);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  }

  function canvasPoint(ev) {
    const canvas = $("map");
    const rect = canvas.getBoundingClientRect();
    const x = (ev.clientX - rect.left) / rect.width * canvas.width;
    const y = (ev.clientY - rect.top) / rect.height * canvas.height;
    return { x, y };
  }

  function clampPan() {
    const min = -2.5;
    const max = 2.5;
    state.ox = Math.min(1024 * max, Math.max(1024 * min, state.ox));
    state.oy = Math.min(512 * max, Math.max(512 * min, state.oy));
  }

  function resetView() {
    state.zoom = 1; state.ox = 0; state.oy = 0;
    drawMap();
  }

  $("zoom-in").addEventListener("click", () => {
    state.zoom = Math.min(8, state.zoom * 1.5);
    clampPan(); drawMap();
  });
  $("zoom-out").addEventListener("click", () => {
    state.zoom = Math.max(1, state.zoom / 1.5);
    if (state.zoom === 1) { state.ox = 0; state.oy = 0; }
    clampPan(); drawMap();
  });
  $("zoom-fit").addEventListener("click", resetView);

  const map = $("map");
  map.addEventListener("wheel", (ev) => {
    if (!state.world) return;
    ev.preventDefault();
    const p = canvasPoint(ev);
    const before = { x: (p.x - state.ox) / state.zoom, y: (p.y - state.oy) / state.zoom };
    const factor = ev.deltaY < 0 ? 1.18 : 1 / 1.18;
    state.zoom = Math.min(8, Math.max(1, state.zoom * factor));
    state.ox = p.x - before.x * state.zoom;
    state.oy = p.y - before.y * state.zoom;
    clampPan();
    drawMap();
  }, { passive: false });

  map.addEventListener("pointerdown", (ev) => {
    if (!state.world) return;
    map.setPointerCapture(ev.pointerId);
    state.drag = { x: ev.clientX, y: ev.clientY, ox: state.ox, oy: state.oy, moved: false };
    map.classList.add("dragging");
  });

  map.addEventListener("pointermove", (ev) => {
    if (!state.drag) return;
    const dx = ev.clientX - state.drag.x;
    const dy = ev.clientY - state.drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 4) state.drag.moved = true;
    const rect = map.getBoundingClientRect();
    const scaleX = 1024 / rect.width;
    const scaleY = 512 / rect.height;
    state.ox = state.drag.ox + dx * scaleX;
    state.oy = state.drag.oy + dy * scaleY;
    clampPan();
    drawMap();
  });

  map.addEventListener("pointerup", (ev) => {
    if (!state.drag) return;
    const moved = state.drag.moved;
    state.drag = null;
    map.classList.remove("dragging");
    if (!moved) inspectAt(ev);
  });

  map.addEventListener("pointercancel", () => {
    state.drag = null;
    map.classList.remove("dragging");
  });

  function inspectAt(ev) {
    if (!state.world) return;
    const p = canvasPoint(ev);
    const x = (p.x - state.ox) / state.zoom;
    const y = (p.y - state.oy) / state.zoom;
    if (x < 0 || y < 0 || x > 1024 || y > 512) return;
    const lat = 90 - (y / 512) * 180;
    const lon = (x / 1024) * 360 - 180;
    getJSON("/api/cell", { id: state.world, lat, lon, year: state.year })
      .then(renderInspector)
      .catch((err) => toast(err.message));
  }

  function row(k, v) {
    return "<dt>" + esc(k) + "</dt><dd>" + esc(v) + "</dd>";
  }

  function renderInspector(info) {
    const owner = info.owner
      ? '<span style="color:' + esc(civColor(info.owner.id)) + '">' + esc(info.owner.name) + "</span>"
      : "unclaimed";
    const events = (info.events || []).map((e) =>
      '<button type="button" class="cause-item event-link" data-event="' + e.id + '">' +
      '<span class="tag">' + esc(e.year) + "</span>" + esc(e.title) + "</button>"
    ).join("");
    $("inspector").className = "";
    $("inspector").innerHTML =
      "<h3>" + esc(info.biomeLabel) + "</h3>" +
      '<div class="where">cell ' + fmt(info.cell) + " · " + Math.abs(info.lat).toFixed(1) + "°" + (info.lat >= 0 ? "N" : "S") +
      " " + Math.abs(info.lon).toFixed(1) + "°" + (info.lon >= 0 ? "E" : "W") + "</div>" +
      '<dl class="kv">' +
      row("owner", "") +
      row("elevation", fmt(info.elevation) + " m") +
      row("temperature", fmt(info.temperature, 1) + " °C") +
      row("rainfall", fmt(info.precip) + " mm") +
      row("discharge", fmt(info.discharge, 1) + " m³/s") +
      row("productivity", fmt(info.npp) + " gC/m²") +
      row("species", fmt(info.species)) +
      row("capacity", fmtBig(info.carryingCapacity) + " people") +
      row("plate", "#" + fmt(info.plate) + " " + info.plateKind) +
      row("stress", info.stress) +
      "</dl>" +
      '<div class="where" style="margin-top:8px">' + owner +
      (info.river ? " · river" : "") + (info.lake ? " · lake" : "") + (info.delta ? " · delta" : "") + "</div>" +
      (events ? '<div class="cause-section"><h3>Recent events here</h3><div class="cause-list">' + events + "</div></div>" : "");
  }

  /* ------------------------------------------------------------- timeline */

  async function loadTimeline() {
    if (!state.world) return;
    try {
      state.timeline = await getJSON("/api/timeline", { id: state.world });
      drawTimeline();
    } catch (_) { /* non-fatal */ }
  }

  function drawTimeline() {
    const canvas = $("timeline");
    const ctx = canvas.getContext("2d");
    const density = state.timeline ? state.timeline.density || [] : [];
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#0c1218";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    if (!density.length) return;
    const max = Math.max(1, ...density.map((d) => d.count));
    const barW = canvas.width / density.length;
    const from = Number(state.filters.from || 0);
    const to = Number(state.filters.to || (state.meta ? state.meta.years : 10000));
    for (let i = 0; i < density.length; i++) {
      const d = density[i];
      const h = Math.max(2, (d.count / max) * (canvas.height - 12));
      const x = i * barW;
      const inRange = d.century * 100 <= to && d.century * 100 + 99 >= from;
      ctx.fillStyle = inRange ? "rgba(86,210,166,0.75)" : "rgba(124,196,255,0.26)";
      ctx.fillRect(x + 1, canvas.height - h - 2, Math.max(1, barW - 2), h);
    }
    ctx.strokeStyle = "rgba(255,255,255,0.07)";
    ctx.beginPath();
    ctx.moveTo(0, canvas.height - 1);
    ctx.lineTo(canvas.width, canvas.height - 1);
    ctx.stroke();
    // Population trend from snapshots, log-scaled.
    const series = state.timeline.series || [];
    if (series.length > 1) {
      const totals = series.map((s) => s.civs.reduce((sum, c) => sum + (c.alive ? c.population : 0), 0));
      const maxPop = Math.max(1, ...totals);
      ctx.strokeStyle = "rgba(240,199,91,0.75)";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      series.forEach((s, i) => {
        const x = (s.year / Math.max(1, state.meta.years)) * canvas.width;
        const y = canvas.height - 8 - (Math.log10(1 + totals[i]) / Math.log10(1 + maxPop)) * (canvas.height - 20);
        if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
      });
      ctx.stroke();
    }
  }

  $("timeline").addEventListener("click", (ev) => {
    if (!state.timeline || !state.meta) return;
    const rect = ev.target.getBoundingClientRect();
    const t = (ev.clientX - rect.left) / rect.width;
    const density = state.timeline.density || [];
    if (!density.length) return;
    const century = Math.min(density.length - 1, Math.max(0, Math.floor(t * density.length)));
    $("filter-from").value = century * 100;
    $("filter-to").value = Math.min(state.meta.years, century * 100 + 99);
    applyFilters();
  });

  /* ------------------------------------------------------------ chronicle */

  function currentFilters() {
    return {
      from: $("filter-from").value,
      to: $("filter-to").value,
      kind: $("filter-kind").value,
      civ: $("filter-civ").value,
      q: $("filter-q").value.trim(),
    };
  }

  function applyFilters() {
    state.filters = currentFilters();
    $("event-list").scrollTop = 0;
    loadChronicle(true);
    drawTimeline();
  }

  $("filter-kind").addEventListener("change", applyFilters);
  $("filter-civ").addEventListener("change", applyFilters);
  $("filter-from").addEventListener("change", applyFilters);
  $("filter-to").addEventListener("change", applyFilters);
  $("filter-q").addEventListener("input", () => {
    clearTimeout(state.searchTimer);
    state.searchTimer = setTimeout(applyFilters, 280);
  });
  $("filter-reset").addEventListener("click", () => {
    $("filter-kind").value = "all";
    $("filter-civ").value = "all";
    $("filter-q").value = "";
    $("filter-from").value = "";
    $("filter-to").value = state.meta ? state.meta.years : "";
    applyFilters();
  });

  async function loadChronicle(reset) {
    if (!state.world) return;
    if (reset) { state.offset = 0; state.events = []; }
    const f = currentFilters();
    const query = { id: state.world, offset: state.offset, limit: 100 };
    if (f.kind && f.kind !== "all") query.kind = f.kind;
    if (f.civ && f.civ !== "all") query.civ = f.civ;
    if (f.from !== "") query.from = f.from;
    if (f.to !== "") query.to = f.to;
    if (f.q) query.q = f.q;
    try {
      const page = await getJSON("/api/chronicle", query);
      state.total = page.total;
      state.events = state.events.concat(page.events);
      state.offset = state.events.length;
      renderEvents();
    } catch (err) {
      toast(err.message);
    }
  }

  function renderEvents() {
    const list = $("event-list");
    list.innerHTML = "";
    $("event-count").textContent = state.events.length + " of " + state.total + " events";
    $("event-more").hidden = state.events.length >= state.total;
    if (!state.events.length) {
      const li = document.createElement("li");
      li.className = "event";
      li.innerHTML = '<span class="year">—</span><span class="title">No events match these filters.</span>';
      list.appendChild(li);
      return;
    }
    const frag = document.createDocumentFragment();
    for (const e of state.events) {
      const li = document.createElement("li");
      li.className = "event" + (state.selected === e.id ? " selected" : "");
      li.dataset.id = e.id;
      li.tabIndex = 0;
      const color = e.civ !== null && e.civ !== undefined ? civColor(e.civ) : "#7cc4ff";
      li.innerHTML =
        '<span class="sev" style="background:' + esc(color) + '"></span>' +
        '<span class="year">' + esc(e.year) + "</span>" +
        '<span class="title">' + esc(e.title) + "</span>" +
        '<span class="meta"><span class="kind">' + esc(e.label || labelForKind(e.kind)) + "</span>" +
        (e.civ !== null && e.civ !== undefined ? "<span>" + esc(civName(e.civ)) + "</span>" : "") +
        "<span>severity " + Number(e.severity || 0).toFixed(2) + "</span></span>";
      li.addEventListener("click", () => loadEvent(e.id));
      li.addEventListener("keydown", (ev) => { if (ev.key === "Enter") loadEvent(e.id); });
      frag.appendChild(li);
    }
    list.appendChild(frag);
  }

  $("load-more").addEventListener("click", () => loadChronicle(false));

  async function loadEvent(id) {
    if (!state.world) return;
    try {
      const detail = await getJSON("/api/event", { id: state.world, event: id });
      state.selected = id;
      renderEvents();
      renderCause(detail);
      $("cause-panel").scrollTop = 0;
    } catch (err) {
      toast(err.message);
    }
  }

  function eventLink(e) {
    return '<button type="button" class="cause-item event-link" data-event="' + e.id + '">' +
      '<span class="tag">' + esc(e.year) + "</span>" + esc(e.title) + "</button>";
  }

  function renderCause(detail) {
    const e = detail.event;
    const causes = (detail.causes || []);
    const chain = detail.chain;
    const paths = detail.paths || [];
    const descendants = detail.descendants || [];
    const color = e.civ !== null && e.civ !== undefined ? civColor(e.civ) : "#7cc4ff";
    let html = "";
    html += '<h3 class="cause-title" style="border-left:3px solid ' + esc(color) + ';padding-left:8px">' + esc(e.title) + "</h3>";
    html += '<div class="cause-where">year ' + esc(e.year) + " · " + esc(e.label || labelForKind(e.kind)) +
      (e.civ !== null && e.civ !== undefined ? " · " + esc(civName(e.civ)) : "") +
      " · severity " + Number(e.severity || 0).toFixed(2) + "</div>";
    html += '<p class="cause-detail">' + esc(detail.detail) + "</p>";
    if (detail.explain) html += '<div class="explain">' + esc(detail.explain) + "</div>";

    html += '<div class="cause-section"><h3>Direct causes</h3><div class="cause-list">';
    if (!causes.length) {
      html += '<span class="muted small">No causes recorded.</span>';
    }
    for (const c of causes) {
      if (c.type === "event") {
        html += '<button type="button" class="cause-item event-link" data-event="' + c.id + '">' +
          '<span class="tag">event</span>' + esc(c.year) + " · " + esc(c.title) + "</button>";
      } else {
        html += '<div class="cause-item state"><span class="tag">state</span>' + esc(c.text) + "</div>";
      }
    }
    html += "</div></div>";

    if (chain && chain.nodes && chain.nodes.length > 1) {
      html += '<div class="cause-section"><h3>Causal chain · depth ' + esc(chain.maxDepth) +
        (chain.truncated ? " (truncated)" : "") + '</h3><div class="chain">';
      for (const n of chain.nodes.slice(0, 40)) {
        html += '<button type="button" class="chain-node event-link' + (n.depth === 0 ? " root" : "") +
          '" data-event="' + n.id + '" style="margin-left:' + Math.min(20, n.depth * 10) + 'px">' +
          '<span class="depth">d' + n.depth + "</span>" + esc(n.year) + " · " + esc(n.title) + "</button>";
      }
      html += "</div></div>";
    }

    if (paths.length) {
      html += '<div class="cause-section"><h3>Paths to physical facts</h3>';
      for (const path of paths.slice(0, 6)) {
        const parts = path.map((step) => {
          if (step.type === "event") {
            return '<button type="button" class="cause-item event-link" style="display:inline-block;width:auto;padding:2px 6px" data-event="' +
              step.id + '">' + esc(step.title) + "</button>";
          }
          return "<span>" + esc(step.text) + "</span>";
        });
        html += '<div class="path">' + parts.join('<span class="arrow">→</span>') + "</div>";
      }
      html += "</div>";
    }

    if (descendants.length) {
      html += '<div class="cause-section"><h3>Later consequences</h3><div class="cause-list">';
      for (const d of descendants.slice(0, 12)) html += eventLink(d);
      html += "</div></div>";
    }

    if (detail.civInfo) {
      html += '<div class="cause-section"><h3>People</h3><div class="small">' +
        esc(detail.civInfo.name) + (detail.civInfo.alive ? " · extant" : " · fallen") + "</div></div>";
    }

    const body = $("cause-body");
    body.className = "";
    body.innerHTML = html;
  }

  $("cause-body").addEventListener("click", (ev) => {
    const btn = ev.target.closest("[data-event]");
    if (!btn) return;
    loadEvent(Number(btn.dataset.event));
  });
  $("inspector").addEventListener("click", (ev) => {
    const btn = ev.target.closest("[data-event]");
    if (!btn) return;
    loadEvent(Number(btn.dataset.event));
    $("cause-panel").scrollIntoView({ behavior: "smooth", block: "start" });
  });

  /* --------------------------------------------------------------- year ui */

  $("year").addEventListener("input", () => {
    state.year = Number($("year").value);
    $("year-label").textContent = fmt(state.year);
    if (state.layer === "political" || state.civOverlay) {
      clearTimeout(state.yearTimer);
      state.yearTimer = setTimeout(() => renderMap(), 140);
    }
  });

  $("civ-overlay").addEventListener("change", () => {
    state.civOverlay = $("civ-overlay").checked;
    renderMap();
  });

  /* --------------------------------------------------------------- export */

  $("download-map").addEventListener("click", async () => {
    if (!state.world) return;
    try {
      const res = await fetch(api("/api/map", mapQuery(1600, 800)));
      const blob = await res.blob();
      const link = document.createElement("a");
      link.href = URL.createObjectURL(blob);
      link.download = (state.meta ? state.meta.planet : "terra") + "-" + state.layer + ".png";
      document.body.appendChild(link);
      link.click();
      link.remove();
      setTimeout(() => URL.revokeObjectURL(link.href), 4000);
    } catch (err) {
      toast("Could not download the map: " + err.message);
    }
  });

  $("export-md").addEventListener("click", () => {
    if (state.world) location.href = api("/api/export/chronicle.md", { id: state.world });
  });
  $("export-json").addEventListener("click", () => {
    if (state.world) location.href = api("/api/export/chronicle.json", { id: state.world });
  });
  $("export-csv").addEventListener("click", () => {
    if (state.world) location.href = api("/api/export/chronicle.csv", { id: state.world });
  });
  $("export-world").addEventListener("click", () => {
    if (state.world) location.href = api("/api/export/world.json", { id: state.world });
  });

  /* ----------------------------------------------------------------- boot */

  async function boot() {
    if (!token) {
      setStatus("Missing Studio token. Open the URL the launcher provided.");
      $("generate").disabled = true;
      return;
    }
    try {
      const health = await getJSON("/api/health");
      $("version").textContent = "terraform " + health.version;
      setStatus("Ready · Terra " + health.version + " · forge a world");
    } catch (err) {
      setStatus("Server unreachable: " + err.message);
    }
    const seedParam = Number(params.get("seed"));
    if (Number.isFinite(seedParam) && seedParam >= 0 && params.has("seed")) {
      $("seed").value = String(seedParam >>> 0);
    }
    const yearsParam = Number(params.get("years"));
    if ([1000, 3000, 6000, 10000, 20000].includes(yearsParam)) {
      $("years").value = String(yearsParam);
    }
    $("year").max = 10000;
    drawMap();
  }

  boot();
})();
