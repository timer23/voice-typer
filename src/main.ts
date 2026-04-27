import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

const micBtn        = document.getElementById("micBtn")!        as HTMLButtonElement;
const statusBar     = document.getElementById("statusBar")!     as HTMLDivElement;
const settingsBtn   = document.getElementById("settingsBtn")!   as HTMLButtonElement;
const settingsPanel = document.getElementById("settingsPanel")! as HTMLDivElement;
const apiKeyInput   = document.getElementById("apiKeyInput")!   as HTMLInputElement;
const saveKeyBtn    = document.getElementById("saveKeyBtn")!    as HTMLButtonElement;
const saveMsg       = document.getElementById("saveMsg")!       as HTMLDivElement;
const closeBtn      = document.getElementById("closeBtn")!      as HTMLButtonElement;
const helpBtn       = document.getElementById("helpBtn")!       as HTMLButtonElement;
const helpTooltip   = document.getElementById("helpTooltip")!   as HTMLDivElement;
const waveWrap      = document.getElementById("waveWrap")!      as HTMLDivElement;
const waveBars      = document.getElementById("waveBars")!      as HTMLDivElement;

const win = getCurrentWindow();
const SETTINGS_HEIGHT = 195;
const DEFAULT_HEIGHT  = 106;

// ── Окно ────────────────────────────────────────────────────────────────────

closeBtn.addEventListener("click", () => win.close());


// ── Настройки ───────────────────────────────────────────────────────────────

let settingsOpen = false;

settingsBtn.addEventListener("click", async () => {
  settingsOpen = !settingsOpen;
  if (settingsOpen) {
    await win.setSize(new LogicalSize(220, SETTINGS_HEIGHT));
    settingsPanel.classList.add("open");
  } else {
    settingsPanel.classList.remove("open");
    await win.setSize(new LogicalSize(220, DEFAULT_HEIGHT));
  }
});


// ── Help tooltip ────────────────────────────────────────────────────────────

let helpVisible = false;

helpBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  helpVisible = !helpVisible;
  helpTooltip.classList.toggle("visible", helpVisible);
});

document.addEventListener("click", () => {
  if (helpVisible) {
    helpVisible = false;
    helpTooltip.classList.remove("visible");
  }
});


// ── API Key ─────────────────────────────────────────────────────────────────

saveKeyBtn.addEventListener("click", async () => {
  const key = apiKeyInput.value.trim();
  if (!key) { showSaveMsg("Введите ключ", true); return; }
  try {
    await invoke("save_api_key", { key });
    showSaveMsg("✓ Сохранено");
  } catch (e) {
    showSaveMsg(String(e), true);
  }
});

function showSaveMsg(text: string, isError = false) {
  saveMsg.textContent = text;
  saveMsg.className = "save-msg" + (isError ? " error" : "");
  if (!isError) setTimeout(() => { saveMsg.textContent = ""; }, 3000);
}

async function loadSavedKey() {
  try {
    const key = await invoke<string>("get_api_key");
    if (key) apiKeyInput.value = key;
  } catch {}
}


// ── Waveform visualization (bar history, 50 bars) ────────────────────────────

const N_BARS = 50;
const barEls: HTMLDivElement[] = [];
let history = new Array(N_BARS).fill(10);

for (let i = 0; i < N_BARS; i++) {
  const b = document.createElement("div");
  b.className = "wave-bar";
  waveBars.appendChild(b);
  barEls.push(b);
}

function applyHistory() {
  for (let i = 0; i < N_BARS; i++) barEls[i].style.height = history[i] + "%";
}

let audioCtx: AudioContext | null = null;
let analyser: AnalyserNode | null = null;
let vizStream: MediaStream | null = null;
let animId: number | null = null;

async function startViz() {
  if (audioCtx) return;
  waveWrap.classList.add("recording");
  try {
    vizStream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
    audioCtx = new AudioContext();
    analyser = audioCtx.createAnalyser();
    analyser.fftSize = 256;
    audioCtx.createMediaStreamSource(vizStream).connect(analyser);

    const data = new Uint8Array(analyser.frequencyBinCount);
    let frame = 0;

    function tick() {
      animId = requestAnimationFrame(tick);
      if (++frame % 3 !== 0) return;
      analyser!.getByteFrequencyData(data);
      const avg = data.reduce((a, b) => a + b, 0) / data.length;
      history.shift();
      history.push(Math.max(10, Math.round((avg / 128) * 100)));
      applyHistory();
    }
    tick();
  } catch {
    // fallback: animated fake bars
    function fakeTick() {
      animId = requestAnimationFrame(fakeTick);
      history.shift();
      history.push(Math.round(Math.random() * 55 + 10));
      applyHistory();
    }
    fakeTick();
  }
}

function stopViz() {
  waveWrap.classList.remove("recording");
  if (animId !== null) { cancelAnimationFrame(animId); animId = null; }
  if (vizStream)       { vizStream.getTracks().forEach(t => t.stop()); vizStream = null; }
  if (audioCtx)        { audioCtx.close().catch(() => {}); audioCtx = null; }
  analyser = null;
  history.fill(10);
  applyHistory();
}


// ── Запись через кнопку ──────────────────────────────────────────────────────

micBtn.addEventListener("click", async () => {
  const recording = await invoke<boolean>("is_recording");
  console.log("[mic click] is_recording =", recording);
  if (recording) {
    try { await invoke("stop_and_transcribe"); } catch (e) { console.error("[stop]", e); setStatus("error"); }
  } else {
    try { await invoke("start_recording"); } catch (e) { console.error("[start]", e); setStatus("error"); }
  }
});


// ── Tauri события от бэкенда ─────────────────────────────────────────────────

listen<boolean>("recording-state", (e) => {
  console.log("[recording-state]", e.payload);
  setStatus(e.payload ? "recording" : "idle");
});
listen<string>("status", (e) => {
  console.log("[status]", e.payload);
  const s = e.payload;
  if (s.startsWith("Распознаю")) setStatus("transcribing");
  else if (s.startsWith("Ошибка"))  setStatus("error");
});
listen<string>("transcription-ready", (e) => {
  console.log("[transcription-ready]", e.payload);
  setStatus("idle");
});

function setStatus(state: "idle" | "recording" | "transcribing" | "error") {
  micBtn.classList.toggle("recording", state === "recording");
  statusBar.className = "status-bar";
  if (state === "recording") {
    statusBar.classList.add("recording");
    startViz();
  } else {
    stopViz();
    if (state === "transcribing") statusBar.classList.add("transcribing");
  }
}

document.addEventListener("keydown", (e) => {
  if (e.key === "F12") invoke("open_devtools");
});


// ── Init ─────────────────────────────────────────────────────────────────────

console.log("[init] Voice Typer started");
loadSavedKey();
applyHistory();
invoke<boolean>("is_recording")
  .then(r => { console.log("[init] is_recording =", r); setStatus(r ? "recording" : "idle"); })
  .catch(e => console.error("[init]", e));
