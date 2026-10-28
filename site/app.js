const kinds = {
  images: {
    title: "Images",
    convert: ["PNG", "WebP", "BMP", "GIF", "TIFF", "PDF"],
    tools: ["Compresser", "1920 px", "1280 px", "Pivoter"],
    convertDetail: "Convertir en PNG",
    toolDetail: "JPEG qualité 70",
  },
  video: {
    title: "Vidéo",
    convert: ["MP4", "MP3", "MKV", "GIF", "WebM", "WMV", "AVI"],
    tools: ["Compresser", "Muet", "Capture"],
    convertDetail: "Convertir en MP4",
    toolDetail: "Vidéo plus légère",
  },
  audio: {
    title: "Audio",
    convert: ["M4A", "WMA", "WAV", "AIFF", "FLAC", "Opus", "OGG"],
    tools: ["Compresser", "Normaliser"],
    convertDetail: "Convertir en M4A",
    toolDetail: "Audio plus léger",
  },
  pdf: {
    title: "PDF",
    convert: ["TXT", "MD", "HTML", "DOCX", "EPUB", "PNG", "JPG", "PPTX", "XLSX"],
    tools: ["Compresser", "Séparer", "Métadonnées"],
    convertDetail: "Convertir en DOCX",
    toolDetail: "Réduire le PDF",
  },
  documents: {
    title: "Documents",
    convert: ["TXT", "MD", "HTML", "PDF", "EPUB"],
    tools: ["Compresser", "Métadonnées"],
    convertDetail: "Convertir en PDF",
    toolDetail: "Réduire le document",
  },
  slides: {
    title: "Diapositives",
    convert: ["PDF", "JPG", "PNG"],
    tools: ["PDF", "JPG", "PNG"],
    convertDetail: "Convertir en PDF",
    toolDetail: "Une image par diapositive",
  },
  sheets: {
    title: "Tableurs",
    convert: ["ODS", "CSV", "TSV", "PDF"],
    tools: ["ODS", "CSV", "TSV", "PDF"],
    convertDetail: "Convertir en CSV",
    toolDetail: "Même liste sur Alt",
  },
  epub: {
    title: "EPUB",
    convert: ["HTML", "TXT", "MD", "PDF", "DOCX"],
    tools: ["HTML", "TXT", "MD", "PDF", "DOCX"],
    convertDetail: "Convertir en HTML",
    toolDetail: "Même liste sur Alt",
  },
  text: {
    title: "Texte",
    convert: ["TXT", "MD", "HTML", "PDF", "DOCX", "EPUB"],
    tools: ["PDF", "DOCX", "EPUB", "MD"],
    convertDetail: "Convertir en PDF",
    toolDetail: "HTML, TXT ou Markdown",
  },
  archives: {
    title: "Archives",
    convert: ["TAR", "GZIP"],
    tools: ["Extraire", "TAR", "GZIP"],
    convertDetail: "Convertir en TAR",
    toolDetail: "Extraire à côté de l'archive",
  },
};

const actions = ["Chemin", "Explorateur", "Terminal", "Copier", "Déplacer"];

const title = document.getElementById("details-title");
const panel = document.getElementById("bento-panel");
const tabs = [...document.querySelectorAll(".bento-tabs button")];
let current = "images";

function wheelSvg(labels, onPick) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 272 272");
  svg.setAttribute("class", "wheel-svg");
  const count = labels.length;
  const cx = 136;
  const cy = 136;
  const outer = 118;
  const inner = 52;
  labels.forEach((label, index) => {
    const start = (index / count) * Math.PI * 2 + 0.035;
    const end = ((index + 1) / count) * Math.PI * 2 - 0.035;
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", segment(cx, cy, outer, inner, start, end));
    path.setAttribute("class", "petal");
    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    const mid = (start + end) / 2;
    const radius = (outer + inner) / 2;
    text.setAttribute("x", String(cx + Math.sin(mid) * radius));
    text.setAttribute("y", String(cy - Math.cos(mid) * radius));
    text.setAttribute("text-anchor", "middle");
    text.setAttribute("dominant-baseline", "middle");
    text.setAttribute("class", count > 6 ? "wheel-label is-small" : "wheel-label");
    text.textContent = label;
    const choose = () => {
      svg.querySelectorAll(".petal, .wheel-label").forEach((node) => node.classList.remove("is-hot"));
      path.classList.add("is-hot");
      text.classList.add("is-hot");
      onPick(label, index);
    };
    path.addEventListener("pointerenter", choose);
    path.addEventListener("click", choose);
    svg.append(path, text);
  });
  const hole = document.createElementNS("http://www.w3.org/2000/svg", "circle");
  hole.setAttribute("cx", "136");
  hole.setAttribute("cy", "136");
  hole.setAttribute("r", "46");
  hole.setAttribute("fill", "#fff");
  svg.append(hole);
  return svg;
}

function segment(cx, cy, outer, inner, start, end) {
  const point = (radius, angle) => [cx + Math.sin(angle) * radius, cy - Math.cos(angle) * radius];
  const [x0, y0] = point(outer, start);
  const [x1, y1] = point(outer, end);
  const [x2, y2] = point(inner, end);
  const [x3, y3] = point(inner, start);
  const large = end - start > Math.PI ? 1 : 0;
  return `M ${x0} ${y0} A ${outer} ${outer} 0 ${large} 1 ${x1} ${y1} L ${x2} ${y2} A ${inner} ${inner} 0 ${large} 0 ${x3} ${y3} Z`;
}

function mount(id, labels, resultId, detail) {
  const host = document.getElementById(id);
  const result = document.getElementById(resultId);
  host.replaceChildren(wheelSvg(labels, (label) => {
    result.textContent = detail(label);
  }));
  const first = host.querySelector(".petal");
  const firstLabel = host.querySelector(".wheel-label");
  if (first && firstLabel) {
    first.classList.add("is-hot");
    firstLabel.classList.add("is-hot");
  }
  result.textContent = detail(labels[0]);
}

function show(kind) {
  current = kind;
  const data = kinds[kind];
  title.textContent = data.title;
  panel.setAttribute("aria-labelledby", `tab-${kind === "sheets" ? "sheets" : kind}`);
  tabs.forEach((tab) => {
    const selected = tab.dataset.kind === kind;
    tab.setAttribute("aria-selected", selected ? "true" : "false");
    tab.tabIndex = selected ? 0 : -1;
  });
  mount("convert-fan", data.convert, "convert-result", (label) => `Convertir en ${label}`);
  mount("tool-fan", data.tools, "tool-result", (label) => label === data.tools[0] ? data.toolDetail : label);
  document.querySelectorAll(".demo-key").forEach((key) => key.classList.remove("is-held"));
}

tabs.forEach((tab) => {
  tab.addEventListener("click", () => show(tab.dataset.kind));
});

mount("action-fan", actions, "action-result", (label) => {
  return {
    Chemin: "Copier le chemin",
    Explorateur: "Afficher dans l'Explorateur",
    Terminal: "Ouvrir le terminal ici",
    Copier: "Copier vers un dossier",
    Déplacer: "Déplacer vers un dossier",
  }[label];
});
show("images");

const imageFormats = new Set(["jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff"]);
const extOf = {
  PNG: "png", WebP: "webp", BMP: "bmp", GIF: "gif", TIFF: "tiff", PDF: "pdf", JPG: "jpg",
  MP4: "mp4", MP3: "mp3", MKV: "mkv", WebM: "webm", WMV: "wmv", AVI: "avi",
  M4A: "m4a", WMA: "wma", WAV: "wav", AIFF: "aiff", FLAC: "flac", Opus: "opus", OGG: "ogg",
  TXT: "txt", MD: "md", HTML: "html", DOCX: "docx", EPUB: "epub", PPTX: "pptx", XLSX: "xlsx",
  ODS: "ods", CSV: "csv", TSV: "tsv", TAR: "tar", GZIP: "gzip",
};
const kindOfFormat = {
  jpg: "images", jpeg: "images", png: "images", webp: "images", bmp: "images", gif: "images", tiff: "images",
  mp3: "audio", m4a: "audio", wav: "audio", flac: "audio", ogg: "audio", opus: "audio", aiff: "audio", wma: "audio",
  mp4: "video", mov: "video", mkv: "video", webm: "video", avi: "video", wmv: "video",
  pdf: "pdf", zip: "archives", tar: "archives", gzip: "archives",
  docx: "documents", pptx: "slides", xlsx: "sheets", ods: "sheets", csv: "sheets", tsv: "sheets",
  epub: "epub", txt: "text", md: "text", html: "text", srt: "text", vtt: "text",
};

const desk = [
  { id: "tasse", name: "tasse.jpg", format: "jpg", x: 27, y: 32, photo: "images/cinder-cup.jpg", selected: true },
  { id: "mixtape", name: "mixtape.mp3", format: "mp3", x: 64, y: 27 },
  { id: "weekend", name: "weekend.mov", format: "mov", x: 78, y: 49 },
  { id: "notes", name: "field-notes.pdf", format: "pdf", x: 44, y: 47 },
  { id: "pluie", name: "pluie.png", format: "png", x: 16, y: 61, photo: "images/cinder-window.jpg" },
  { id: "carnet", name: "carnet.jpg", format: "jpg", x: 61, y: 67, photo: "images/cinder-desk.jpg" },
  { id: "project", name: "little-project.zip", format: "zip", x: 33, y: 76 },
  { id: "something", name: "notes.png", format: "png", x: 85, y: 76 },
];

const filesEl = document.getElementById("files");
const hint = document.getElementById("gesture-hint");
const hintPath = document.getElementById("hint-path");
const ghost = document.getElementById("drag-ghost");
const shiftPrompt = document.getElementById("drag-shift-prompt");
const menu = document.getElementById("radial-menu");
let drag = null;
let hintGone = false;

function iconSrc(file) {
  if (file.photo && imageFormats.has(file.format)) return file.photo;
  return `icons/${file.format}.png`;
}

function renderDesk() {
  filesEl.replaceChildren(...desk.map((file) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file${file.selected ? " is-selected" : ""}${file.born ? " is-born" : ""}${drag && drag.id === file.id && drag.moved ? " is-source" : ""}`;
    button.style.setProperty("--x", `${file.x}%`);
    button.style.setProperty("--y", `${file.y}%`);
    button.dataset.id = file.id;
    const art = document.createElement("span");
    art.className = "file-art";
    const image = document.createElement("img");
    image.src = iconSrc(file);
    image.alt = "";
    image.draggable = false;
    if (file.photo && imageFormats.has(file.format)) image.className = "is-photo";
    art.append(image);
    const name = document.createElement("span");
    name.className = "file-name";
    name.textContent = file.name;
    button.append(art, name);
    button.addEventListener("pointerdown", (event) => startDrag(event, file));
    return button;
  }));
  desk.forEach((file) => { file.born = false; });
}

function placeHint() {
  if (hintGone || !desk[0]) return;
  const file = desk[0];
  const filesBox = filesEl.getBoundingClientRect();
  const wrap = hint.parentElement.getBoundingClientRect();
  const x = filesBox.left - wrap.left + (filesBox.width * file.x) / 100;
  const y = filesBox.top - wrap.top + (filesBox.height * file.y) / 100;
  const left = Math.max(8, Math.min(wrap.width - 200, x - 36));
  const top = Math.max(8, y - 168);
  hint.style.left = `${left}px`;
  hint.style.top = `${top}px`;
  const tipX = x - left;
  const tipY = y - top - 6;
  hintPath.setAttribute("d", `M150 16C156 52 ${tipX + 36} ${tipY - 24} ${tipX} ${tipY}M${tipX} ${tipY}l8 -14M${tipX} ${tipY}l16 1`);
}

function targetsFor(file) {
  const kind = kinds[kindOfFormat[file.format] || "images"];
  return kind.convert.filter((label) => extOf[label] && extOf[label] !== file.format);
}

function startDrag(event, file) {
  if (event.button !== undefined && event.button !== 0) return;
  desk.forEach((item) => { item.selected = item.id === file.id; });
  drag = {
    id: file.id,
    pointer: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    cx: event.clientX,
    cy: event.clientY,
    moved: false,
    wheel: false,
    hot: -1,
    labels: targetsFor(file),
  };
  filesEl.querySelectorAll(".file").forEach((button) => {
    button.classList.toggle("is-selected", button.dataset.id === file.id);
  });
  event.currentTarget.setPointerCapture(event.pointerId);
  event.preventDefault();
}

function pointDrag(event) {
  if (!drag || event.pointerId !== drag.pointer) return;
  const distance = Math.hypot(event.clientX - drag.x, event.clientY - drag.y);
  if (!drag.moved && distance < 6) return;
  if (!drag.moved) {
    drag.moved = true;
    hintGone = true;
    hint.hidden = true;
    document.body.classList.add("is-dragging");
    const source = filesEl.querySelector(`[data-id="${drag.id}"]`);
    if (source) source.classList.add("is-source");
    const file = desk.find((item) => item.id === drag.id);
    ghost.replaceChildren();
    const image = document.createElement("img");
    image.src = iconSrc(file);
    image.alt = "";
    const name = document.createElement("span");
    name.textContent = file.name;
    ghost.append(image, name);
    ghost.hidden = false;
  }
  drag.cx = event.clientX;
  drag.cy = event.clientY;
  ghost.style.left = `${event.clientX}px`;
  ghost.style.top = `${event.clientY}px`;
  if (event.pointerType === "touch" && !drag.wheel) openWheel();
  if (drag.wheel) {
    markHot(hotIndex(event));
  } else {
    shiftPrompt.hidden = false;
    shiftPrompt.style.left = `${event.clientX}px`;
    shiftPrompt.style.top = `${event.clientY - 36}px`;
  }
}

function openWheel() {
  if (!drag || drag.wheel || drag.labels.length === 0) return;
  drag.wheel = true;
  shiftPrompt.hidden = true;
  menu.replaceChildren(wheelSvg(drag.labels, () => {}));
  menu.hidden = false;
  menu.style.left = `${drag.cx}px`;
  menu.style.top = `${drag.cy}px`;
}

function hotIndex(event) {
  const rect = menu.getBoundingClientRect();
  const cx = rect.left + rect.width / 2;
  const cy = rect.top + rect.height / 2;
  const dx = event.clientX - cx;
  const dy = event.clientY - cy;
  if (Math.hypot(dx, dy) < 50) return -1;
  let angle = Math.atan2(dx, -dy);
  if (angle < 0) angle += Math.PI * 2;
  return Math.min(drag.labels.length - 1, Math.floor((angle / (Math.PI * 2)) * drag.labels.length));
}

function markHot(index) {
  drag.hot = index;
  const petals = menu.querySelectorAll(".petal");
  const labels = menu.querySelectorAll(".wheel-label");
  petals.forEach((node, i) => node.classList.toggle("is-hot", i === index));
  labels.forEach((node, i) => node.classList.toggle("is-hot", i === index));
}

function endDrag(event) {
  if (!drag || (event && event.pointerId !== undefined && event.pointerId !== drag.pointer)) return;
  const file = desk.find((item) => item.id === drag.id);
  const hot = drag.hot;
  const moved = drag.moved;
  const wheel = drag.wheel;
  if (file && moved) {
    const box = filesEl.getBoundingClientRect();
    const x = ((event.clientX - box.left) / box.width) * 100;
    const y = ((event.clientY - box.top) / box.height) * 100;
    if (wheel && hot >= 0) {
      const ext = extOf[drag.labels[hot]];
      const base = file.name.replace(/\.[^.]+$/, "");
      let name = `${base}.${ext}`;
      let count = 2;
      while (desk.some((item) => item.name === name)) {
        name = `${base}-${count}.${ext}`;
        count += 1;
      }
      desk.forEach((item) => { item.selected = false; });
      desk.push({
        id: `born-${Date.now()}`,
        name,
        format: ext,
        x: Math.min(92, Math.max(8, x + 7)),
        y: Math.min(88, Math.max(14, y + 9)),
        photo: imageFormats.has(ext) ? file.photo : undefined,
        selected: true,
        born: true,
      });
    } else if (!wheel) {
      file.x = Math.min(92, Math.max(8, x));
      file.y = Math.min(88, Math.max(12, y));
    }
  }
  drag = null;
  ghost.hidden = true;
  shiftPrompt.hidden = true;
  menu.hidden = true;
  document.body.classList.remove("is-dragging");
  renderDesk();
  placeHint();
}

window.addEventListener("pointermove", pointDrag);
window.addEventListener("pointerup", endDrag);
window.addEventListener("pointercancel", endDrag);
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && drag) {
    drag.moved = false;
    drag.wheel = false;
    endDrag({ pointerId: drag.pointer, clientX: drag.x, clientY: drag.y });
    return;
  }
  if (!drag || !drag.moved || event.repeat) return;
  if (event.key === "Shift" || event.key === "Alt") openWheel();
});
window.addEventListener("resize", placeHint);

renderDesk();
window.addEventListener("load", placeHint);
requestAnimationFrame(placeHint);
