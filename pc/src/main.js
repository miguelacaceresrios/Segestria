const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const STAGE_LABELS = {
  checking_java: "Revisando Java",
  downloading_client: "Descargando Minecraft",
  downloading_libraries: "Descargando librerías",
  downloading_assets: "Descargando assets",
  installing_forge: "Instalando Forge",
  syncing_mods: "Sincronizando mods",
  launching: "Iniciando",
};

const SERVER_PING_INTERVAL_MS = 15000;

let usernameEl,
  serverEl,
  noServerToggle,
  playButton,
  progressWrap,
  progressFill,
  statusText,
  errorText,
  extraJvmEl,
  serverStatusDot,
  serverStatusTitle,
  serverStatusText,
  serverIconImg,
  serverIconFallback,
  newsSection,
  newsBody;

let pingTimer = null;

function splitServer(value) {
  const trimmed = value.trim();
  if (!trimmed) return { ip: "", port: 25565 };
  const [ip, port] = trimmed.split(":");
  return { ip, port: port ? parseInt(port, 10) : 25565 };
}

function setBusy(busy) {
  playButton.disabled = busy;
  usernameEl.disabled = busy;
  serverEl.disabled = busy;
  progressWrap.classList.toggle("hidden", !busy);
}

function showError(message) {
  errorText.textContent = message;
  errorText.classList.remove("hidden");
}

function currentSettings() {
  const { ip, port } = noServerToggle.checked ? { ip: "", port: 25565 } : splitServer(serverEl.value);
  return {
    username: usernameEl.value.trim(),
    server_ip: ip,
    server_port: port,
    // null = el core elige la RAM óptima según la PC (ver profile::suggested_ram_mb).
    allocated_ram_mb: null,
    extra_jvm_args: extraJvmEl.value.trim(),
  };
}

async function loadSettings() {
  const settings = await invoke("get_settings");
  usernameEl.value = settings.username ?? "";
  serverEl.value = settings.server_ip ? `${settings.server_ip}:${settings.server_port}` : "";
  extraJvmEl.value = settings.extra_jvm_args ?? "";
  setNoServerMode(!settings.server_ip);
}

function setServerIcon(dataUri) {
  if (dataUri) {
    serverIconImg.src = dataUri;
    serverIconImg.classList.remove("hidden");
    serverIconFallback.classList.add("hidden");
  } else {
    serverIconImg.classList.add("hidden");
    serverIconFallback.classList.remove("hidden");
  }
}

function renderServerStatus(status) {
  serverStatusDot.classList.remove("status-dot--online", "status-dot--offline", "status-dot--unknown");
  setServerIcon(status.favicon);
  if (status.online) {
    serverStatusDot.classList.add("status-dot--online");
    serverStatusTitle.textContent = status.motd || "Servidor";
    serverStatusText.textContent = `Online — ${status.players_online}/${status.players_max} jugadores`;
  } else {
    serverStatusDot.classList.add("status-dot--offline");
    serverStatusTitle.textContent = "Servidor";
    serverStatusText.textContent = "Offline o no responde";
  }
}

function setNoServerMode(enabled) {
  noServerToggle.checked = enabled;
  serverEl.disabled = enabled;

  if (!enabled) {
    scheduleServerPing();
    return;
  }

  if (pingTimer) clearInterval(pingTimer);
  serverStatusDot.classList.remove("status-dot--online", "status-dot--offline");
  serverStatusDot.classList.add("status-dot--unknown");
  serverIconFallback.textContent = "S";
  setServerIcon(null);
  serverStatusTitle.textContent = "Sin servidor";
  serverStatusText.textContent = "Vas a jugar sin conectarte a un server";
}

async function refreshServerStatus() {
  const { ip, port } = splitServer(serverEl.value);
  if (!ip) {
    serverStatusDot.classList.remove("status-dot--online", "status-dot--offline");
    serverStatusDot.classList.add("status-dot--unknown");
    serverIconFallback.textContent = "?";
    setServerIcon(null);
    serverStatusTitle.textContent = "Sin servidor";
    serverStatusText.textContent = "Configurá la IP del server";
    return;
  }
  serverIconFallback.textContent = ip[0].toUpperCase();
  const status = await invoke("ping_server", { ip, port });
  renderServerStatus(status);
}

function scheduleServerPing() {
  if (pingTimer) clearInterval(pingTimer);
  refreshServerStatus();
  pingTimer = setInterval(refreshServerStatus, SERVER_PING_INTERVAL_MS);
}

async function loadNews() {
  try {
    const feed = await invoke("get_news");
    if (!feed.items || feed.items.length === 0) return;
    newsBody.innerHTML = "";
    for (const item of feed.items) {
      const el = document.createElement("div");
      el.className = "news-item";
      el.innerHTML = `<div class="news-item-title">${item.title}</div><div class="news-item-date">${item.date}</div><p class="news-item-body">${item.body}</p>`;
      newsBody.appendChild(el);
    }
    newsSection.classList.remove("hidden");
  } catch (err) {
    console.error("no se pudieron cargar las novedades", err);
  }
}

async function play() {
  errorText.classList.add("hidden");

  if (!usernameEl.value.trim()) {
    showError("Poné un nombre de usuario.");
    return;
  }

  const settings = currentSettings();

  setBusy(true);
  statusText.textContent = "Arrancando...";
  progressFill.style.width = "0%";

  try {
    await invoke("play", { settings });
    statusText.textContent = "¡Listo! Minecraft debería abrirse solo.";
  } catch (err) {
    showError(typeof err === "string" ? err : "Algo falló al lanzar el juego.");
  } finally {
    setBusy(false);
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  usernameEl = document.querySelector("#username");
  serverEl = document.querySelector("#server");
  noServerToggle = document.querySelector("#no-server-toggle");
  playButton = document.querySelector("#play-button");
  progressWrap = document.querySelector("#progress-wrap");
  progressFill = document.querySelector("#progress-fill");
  statusText = document.querySelector("#status-text");
  errorText = document.querySelector("#error-text");
  extraJvmEl = document.querySelector("#extra-jvm");
  serverStatusDot = document.querySelector("#server-status-dot");
  serverStatusTitle = document.querySelector("#server-status-title");
  serverStatusText = document.querySelector("#server-status-text");
  serverIconImg = document.querySelector("#server-icon");
  serverIconFallback = document.querySelector("#server-icon-fallback");
  newsSection = document.querySelector("#news");
  newsBody = document.querySelector("#news-body");

  playButton.addEventListener("click", play);
  serverEl.addEventListener("change", scheduleServerPing);
  noServerToggle.addEventListener("change", () => setNoServerMode(noServerToggle.checked));

  await listen("progress", (event) => {
    const { stage, message, fraction } = event.payload;
    statusText.textContent = message ?? STAGE_LABELS[stage] ?? stage;
    if (typeof fraction === "number") {
      progressFill.style.width = `${Math.round(fraction * 100)}%`;
    }
  });

  try {
    await loadSettings();
  } catch (err) {
    console.error("no se pudieron cargar los settings guardados", err);
    setNoServerMode(!serverEl.value);
  }

  loadNews();
});
