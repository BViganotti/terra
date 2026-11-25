/* Terra Studio client. Original work, no external dependencies. */
(function () {
  "use strict";

  const params = new URLSearchParams(location.search);
  const token = params.get("t") || "";
  const api = (path) => path + (path.includes("?") ? "&" : "?") + "t=" + encodeURIComponent(token);

  const $ = (id) => document.getElementById(id);
  const state = { world: null, layer: "atlas", cache: new Map() };

  async function getJSON(path) {
    const res = await fetch(api(path));
    if (!res.ok) throw new Error("request failed: " + res.status);
    return res.json();
  }

  function setStats(pairs) {
    const dl = $("world-stats");
    dl.innerHTML = "";
    for (const [k, v] of pairs) {
      const dt = document.createElement("dt");
      dt.textContent = k;
      const dd = document.createElement("dd");
      dd.textContent = String(v);
      dl.append(dt, dd);
    }
  }

  async function boot() {
    try {
      const health = await getJSON("/api/health");
      $("stage-label").textContent = "ready · terra " + health.version;
    } catch (err) {
      setStats([["status", "server unreachable"]]);
    }
  }

  $("generate").addEventListener("click", async () => {
    const seed = Number($("seed").value) >>> 0;
    const years = Number($("years").value);
    $("progress").hidden = false;
    $("bar").style.width = "4%";
    try {
      const job = await fetch(api("/api/run"), {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ seed, years }),
      }).then((r) => r.json());
      state.world = job.id;
      setStats([["seed", seed], ["years", years], ["job", job.id]]);
      $("bar").style.width = "100%";
    } catch (err) {
      setStats([["error", String(err)]]);
    }
  });

  boot();
})();
