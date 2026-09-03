import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

// ---------- 状態 ----------
const state = {
  repoPath: null,
  branch: null,
  head: null,
  deployedHash: null,
  files: [], // { path, status }
  profiles: [],
  currentProfileId: null,
  connected: false,
  editingProfileId: null,
};

const $ = (id) => document.getElementById(id);
const el = {
  profileSelect: $("profileSelect"),
  addProfileBtn: $("addProfileBtn"),
  editProfileBtn: $("editProfileBtn"),
  pickRepoBtn: $("pickRepoBtn"),
  repoPath: $("repoPath"),
  branchBadge: $("branchBadge"),
  refreshBtn: $("refreshBtn"),
  connectBtn: $("connectBtn"),
  deployBtn: $("deployBtn"),
  moreBtn: $("moreBtn"),
  moreMenu: $("moreMenu"),
  initBtn: $("initBtn"),
  catchupBtn: $("catchupBtn"),
  diffList: $("diffList"),
  remoteTree: $("remoteTree"),
  deployedHash: $("deployedHash"),
  remoteStatus: $("remoteStatus"),
  selectAll: $("selectAll"),
  statusText: $("statusText"),
  progressWrap: $("progressWrap"),
  progressBar: $("progressBar"),
  // modal
  modal: $("profileModal"),
  modalTitle: $("modalTitle"),
  f_name: $("f_name"),
  f_protocol: $("f_protocol"),
  f_host: $("f_host"),
  f_port: $("f_port"),
  f_user: $("f_user"),
  f_pass: $("f_pass"),
  f_remote: $("f_remote"),
  saveProfileBtn: $("saveProfileBtn"),
  cancelProfileBtn: $("cancelProfileBtn"),
  deleteProfileBtn: $("deleteProfileBtn"),
  browseRemoteBtn: $("browseRemoteBtn"),
  // ディレクトリ選択
  pickerModal: $("pickerModal"),
  pickerPath: $("pickerPath"),
  pickerList: $("pickerList"),
  pickerUpBtn: $("pickerUpBtn"),
  pickerCancelBtn: $("pickerCancelBtn"),
  pickerSelectBtn: $("pickerSelectBtn"),
};

// パス操作ヘルパー（Rust側と同じ POSIX 結合ルール）
function joinPath(base, name) {
  const b = base.replace(/\/+$/, "");
  const n = name.replace(/^\/+/, "");
  return b === "" ? `/${n}` : `${b}/${n}`;
}
function parentPath(path) {
  const p = path.replace(/\/+$/, "");
  const idx = p.lastIndexOf("/");
  return idx <= 0 ? "/" : p.slice(0, idx);
}

function setStatus(text, kind = "") {
  el.statusText.textContent = text;
  el.statusText.style.color =
    kind === "error"
      ? "var(--danger)"
      : kind === "ok"
      ? "var(--ok)"
      : "var(--text-dim)";
}

function shortHash(h) {
  return h ? h.slice(0, 7) : "—";
}

// ---------- プロファイル ----------
async function loadProfiles() {
  state.profiles = await invoke("list_profiles");
  renderProfileSelect();
}

function renderProfileSelect() {
  el.profileSelect.innerHTML = "";
  if (state.profiles.length === 0) {
    const opt = document.createElement("option");
    opt.textContent = "プロファイルなし";
    opt.value = "";
    el.profileSelect.appendChild(opt);
    state.currentProfileId = null;
  } else {
    for (const p of state.profiles) {
      const opt = document.createElement("option");
      opt.value = p.id;
      opt.textContent = `${p.name} (${p.protocol.toUpperCase()})`;
      el.profileSelect.appendChild(opt);
    }
    if (!state.profiles.find((p) => p.id === state.currentProfileId)) {
      state.currentProfileId = state.profiles[0].id;
    }
    el.profileSelect.value = state.currentProfileId;
  }
  updateButtons();
}

function currentProfile() {
  return state.profiles.find((p) => p.id === state.currentProfileId) || null;
}

function openProfileModal(profile) {
  state.editingProfileId = profile ? profile.id : null;
  el.modalTitle.textContent = profile ? "プロファイルを編集" : "新規プロファイル";
  el.f_name.value = profile?.name ?? "";
  el.f_protocol.value = profile?.protocol ?? "ftp";
  el.f_host.value = profile?.host ?? "";
  el.f_port.value = profile?.port ?? "";
  el.f_user.value = profile?.user ?? "";
  el.f_pass.value = profile?.password ?? "";
  el.f_remote.value = profile?.remote_root ?? "/";
  el.deleteProfileBtn.hidden = !profile;
  el.modal.hidden = false;
  el.f_name.focus();
}

function defaultPort(proto) {
  return proto === "sftp" ? 22 : 21;
}

async function saveProfile() {
  const proto = el.f_protocol.value;
  const profile = {
    // 新規作成時は空文字。null を送ると Rust 側のデシリアライズに失敗するため。
    id: state.editingProfileId || "",
    name: el.f_name.value.trim() || "無題",
    protocol: proto,
    host: el.f_host.value.trim(),
    port: parseInt(el.f_port.value, 10) || defaultPort(proto),
    user: el.f_user.value.trim(),
    password: el.f_pass.value,
    remote_root: el.f_remote.value.trim() || "/",
  };
  if (!profile.host) {
    setStatus("ホストを入力してください", "error");
    return;
  }
  try {
    const saved = await invoke("save_profile", { profile });
    state.currentProfileId = saved.id;
    el.modal.hidden = true;
    await loadProfiles();
    setStatus("プロファイルを保存しました", "ok");
  } catch (e) {
    setStatus(`保存に失敗: ${e}`, "error");
  }
}

async function deleteProfile() {
  if (!state.editingProfileId) return;
  try {
    await invoke("delete_profile", { id: state.editingProfileId });
    el.modal.hidden = true;
    state.connected = false;
    await loadProfiles();
    setStatus("プロファイルを削除しました");
  } catch (e) {
    setStatus(`削除に失敗: ${e}`, "error");
  }
}

// ---------- リモートフォルダ選択（参照…） ----------
let pickerCurrentPath = "/";

// 入力中のフォーム値から一時プロファイルを作る（未保存でも接続できるように）
function formProfile() {
  const proto = el.f_protocol.value;
  return {
    id: "",
    name: el.f_name.value.trim() || "tmp",
    protocol: proto,
    host: el.f_host.value.trim(),
    port: parseInt(el.f_port.value, 10) || defaultPort(proto),
    user: el.f_user.value.trim(),
    password: el.f_pass.value,
    remote_root: "/",
  };
}

async function openPicker() {
  const prof = formProfile();
  if (!prof.host) {
    setStatus("参照するにはホストを入力してください", "error");
    return;
  }
  // 現在入力されているパスから開始（空なら /）
  pickerCurrentPath = el.f_remote.value.trim() || "/";
  if (!pickerCurrentPath.startsWith("/")) pickerCurrentPath = "/" + pickerCurrentPath;
  el.pickerModal.hidden = false;
  await loadPicker();
}

async function loadPicker() {
  el.pickerPath.textContent = pickerCurrentPath;
  el.pickerList.innerHTML = `<div class="tree-loading">読み込み中…</div>`;
  const prof = formProfile();
  try {
    const dirs = await invoke("remote_browse", {
      profile: prof,
      path: pickerCurrentPath,
    });
    el.pickerList.innerHTML = "";
    if (dirs.length === 0) {
      el.pickerList.innerHTML = `<div class="picker-empty">（サブフォルダはありません）</div>`;
      return;
    }
    for (const d of dirs) {
      const row = document.createElement("div");
      row.className = "picker-row";
      row.innerHTML = `<span class="tree-icon">📁</span><span class="path-text">${d.name}</span><span class="tree-caret">▶</span>`;
      row.addEventListener("click", async () => {
        pickerCurrentPath = joinPath(pickerCurrentPath, d.name);
        await loadPicker();
      });
      el.pickerList.appendChild(row);
    }
  } catch (e) {
    el.pickerList.innerHTML = `<div class="picker-empty" style="color:var(--danger)">接続/一覧エラー: ${e}</div>`;
  }
}

function pickerUp() {
  pickerCurrentPath = parentPath(pickerCurrentPath);
  loadPicker();
}

function pickerSelect() {
  el.f_remote.value = pickerCurrentPath;
  el.pickerModal.hidden = true;
  setStatus(`ルートパスを ${pickerCurrentPath} に設定しました`, "ok");
}

// ---------- リポジトリ / Git ----------
async function pickRepo() {
  const selected = await open({ directory: true, title: "Gitリポジトリを選択" });
  if (!selected) return;
  state.repoPath = selected;
  el.repoPath.textContent = selected;
  el.repoPath.setAttribute("title", selected);
  await refreshGit();
}

async function refreshGit() {
  if (!state.repoPath) return;
  setStatus("Git差分を計算中…");
  try {
    // 先にサーバーの最終デプロイハッシュを取得（接続済みなら）
    if (state.connected && currentProfile()) {
      try {
        state.deployedHash = await invoke("remote_deployed_hash", {
          id: state.currentProfileId,
        });
      } catch (e) {
        state.deployedHash = null;
      }
    }
    const res = await invoke("git_status", {
      repoPath: state.repoPath,
      deployedHash: state.deployedHash,
    });
    state.branch = res.branch;
    state.head = res.head;
    state.files = res.files;
    el.branchBadge.hidden = false;
    el.branchBadge.textContent = `⎇ ${res.branch} · ${shortHash(res.head)}`;
    el.deployedHash.textContent = state.deployedHash
      ? `deployed: ${shortHash(state.deployedHash)}`
      : "未デプロイ";
    renderDiff();
    const ignoredMsg =
      res.ignored > 0 ? `（.git-ftp-ignore で ${res.ignored} 件除外）` : "";
    setStatus(`${res.files.length} 件の変更${ignoredMsg}`, "ok");
  } catch (e) {
    setStatus(`Gitエラー: ${e}`, "error");
    el.diffList.innerHTML = `<div class="empty-state"><div class="empty-emoji">⚠️</div><p>${e}</p></div>`;
  }
  updateButtons();
}

const STATUS_LABEL = {
  A: "追加",
  M: "変更",
  D: "削除",
  R: "改名",
  U: "未追跡",
};

function renderDiff() {
  if (state.files.length === 0) {
    el.diffList.innerHTML = `<div class="empty-state"><div class="empty-emoji">✅</div><p>変更はありません</p></div>`;
    el.selectAll.checked = false;
    return;
  }
  el.diffList.innerHTML = "";
  for (const f of state.files) {
    const row = document.createElement("div");
    row.className = "diff-row" + (f.dirty ? " dirty" : "");
    const st = f.status;
    const dirtyBadge = f.dirty
      ? `<span class="uncommitted-badge" title="作業ツリーの内容がコミット済み内容と異なります（未コミットの変更を含む）">未コミット</span>`
      : "";
    row.innerHTML = `
      <input type="checkbox" class="file-chk" ${f.selected ? "checked" : ""} />
      <span class="status-tag status-${st}" title="${STATUS_LABEL[st] || st}">${st}</span>
      <span class="path-text">${f.path}</span>
      ${dirtyBadge}
    `;
    const chk = row.querySelector(".file-chk");
    chk.addEventListener("change", () => {
      f.selected = chk.checked;
      row.classList.toggle("selected", chk.checked);
      updateSelectAll();
      updateButtons();
    });
    row.classList.toggle("selected", !!f.selected);
    el.diffList.appendChild(row);
  }
}

function updateSelectAll() {
  const sel = state.files.filter((f) => f.selected).length;
  el.selectAll.checked = sel > 0 && sel === state.files.length;
  el.selectAll.indeterminate = sel > 0 && sel < state.files.length;
}

// ---------- リモート接続 / ツリー ----------
async function connectRemote() {
  const prof = currentProfile();
  if (!prof) {
    setStatus("プロファイルを選択してください", "error");
    return;
  }
  setStatus(`${prof.host} に接続中…`);
  el.remoteStatus.textContent = "接続中…";
  try {
    await invoke("remote_connect_test", { id: prof.id });
    state.connected = true;
    el.remoteStatus.textContent = "接続済み";
    el.remoteStatus.style.color = "var(--ok)";
    setStatus(`${prof.host} に接続しました`, "ok");
    await refreshGit(); // deployedHash を取り直す
    await loadRemoteRoot();
  } catch (e) {
    state.connected = false;
    el.remoteStatus.textContent = "接続失敗";
    el.remoteStatus.style.color = "var(--danger)";
    setStatus(`接続エラー: ${e}`, "error");
  }
  updateButtons();
}

async function loadRemoteRoot() {
  const prof = currentProfile();
  el.remoteTree.innerHTML = "";
  const rootUl = document.createElement("div");
  rootUl.className = "tree-children";
  el.remoteTree.appendChild(rootUl);
  await loadTreeInto(rootUl, prof.remote_root, 0);
}

async function loadTreeInto(container, path, depth) {
  const loading = document.createElement("div");
  loading.className = "tree-loading";
  loading.textContent = "読み込み中…";
  container.appendChild(loading);
  try {
    const entries = await invoke("remote_list", {
      id: state.currentProfileId,
      path,
    });
    container.removeChild(loading);
    // ディレクトリ→ファイルの順、名前順
    entries.sort(
      (a, b) => b.is_dir - a.is_dir || a.name.localeCompare(b.name)
    );
    for (const entry of entries) {
      container.appendChild(makeTreeRow(entry, depth));
    }
    if (entries.length === 0) {
      const empty = document.createElement("div");
      empty.className = "tree-loading";
      empty.textContent = "(空)";
      container.appendChild(empty);
    }
  } catch (e) {
    loading.textContent = `エラー: ${e}`;
    loading.style.color = "var(--danger)";
  }
}

function makeTreeRow(entry, depth) {
  const wrap = document.createElement("div");
  const row = document.createElement("div");
  row.className = "tree-row";
  row.style.paddingLeft = `${10 + depth * 16}px`;

  const caret = document.createElement("span");
  caret.className = "tree-caret";
  caret.textContent = entry.is_dir ? "▶" : "";

  const icon = document.createElement("span");
  icon.className = "tree-icon";
  icon.textContent = entry.is_dir ? "📁" : fileEmoji(entry.name);

  const name = document.createElement("span");
  name.className = "path-text";
  name.textContent = entry.name;

  row.append(caret, icon, name);
  wrap.appendChild(row);

  if (entry.is_dir) {
    const children = document.createElement("div");
    children.className = "tree-children";
    children.hidden = true;
    let loaded = false;
    wrap.appendChild(children);
    row.addEventListener("click", async () => {
      const willOpen = children.hidden;
      children.hidden = !willOpen;
      caret.classList.toggle("open", willOpen);
      if (willOpen && !loaded) {
        loaded = true;
        await loadTreeInto(children, entry.path, depth + 1);
      }
    });
  }
  return wrap;
}

function fileEmoji(name) {
  const ext = name.split(".").pop().toLowerCase();
  const map = {
    js: "📜", ts: "📜", json: "🗂", html: "🌐", css: "🎨",
    php: "🐘", py: "🐍", rs: "🦀", md: "📝", png: "🖼", jpg: "🖼",
    jpeg: "🖼", gif: "🖼", svg: "🖼", log: "📄", txt: "📄",
  };
  return map[ext] || "📄";
}

// ---------- デプロイ ----------
async function deploy() {
  const prof = currentProfile();
  const selected = state.files.filter((f) => f.selected);
  if (!prof || selected.length === 0) return;

  const ok = window.confirm(
    `${selected.length} 件のファイルを「${prof.name}」(${prof.host}) にアップロードします。\nよろしいですか?`
  );
  if (!ok) return;

  el.deployBtn.disabled = true;
  el.progressWrap.hidden = false;
  el.progressBar.style.width = "0%";
  setStatus("アップロード中…");

  try {
    const result = await invoke("deploy", {
      id: prof.id,
      repoPath: state.repoPath,
      files: selected.map((f) => ({ path: f.path, status: f.status })),
      headHash: state.head,
    });
    setStatus(
      `完了: ${result.uploaded} アップロード / ${result.deleted} 削除`,
      "ok"
    );
    state.deployedHash = state.head;
    // 反映後に差分を再計算
    await refreshGit();
    if (state.connected) await loadRemoteRoot();
  } catch (e) {
    setStatus(`デプロイ失敗: ${e}`, "error");
  } finally {
    el.progressWrap.hidden = true;
    updateButtons();
  }
}

// ---------- git ftp init / catchup ----------
function closeMenu() {
  el.moreMenu.hidden = true;
}

// init: HEADの全追跡ファイルを初回アップロード（必ず確認ダイアログ）
async function gitFtpInit() {
  closeMenu();
  const prof = currentProfile();
  if (!prof || !state.repoPath) return;

  // 確認用にアップロード対象数を取得
  let count = 0;
  try {
    const files = await invoke("git_tracked_files", {
      repoPath: state.repoPath,
    });
    count = files.length;
  } catch (e) {
    setStatus(`initの準備に失敗: ${e}`, "error");
    return;
  }

  // ★ init は必ず確認ダイアログを出す
  const ok = window.confirm(
    "【git-ftp init】\n\n" +
      `HEADの全追跡ファイル ${count} 件を\n「${prof.name}」(${prof.host}) にアップロードします。\n\n` +
      "サーバー上に同名ファイルがある場合は上書きされます。\n" +
      "通常はサーバーが空の「初回デプロイ」時に使用します。\n\n" +
      "実行してよろしいですか?"
  );
  if (!ok) {
    setStatus("initをキャンセルしました");
    return;
  }

  el.progressWrap.hidden = false;
  el.progressBar.style.width = "0%";
  setStatus("init: 全ファイルをアップロード中…");
  try {
    const result = await invoke("git_ftp_init", {
      id: prof.id,
      repoPath: state.repoPath,
    });
    setStatus(`init 完了: ${result.uploaded} ファイルをアップロード`, "ok");
    await refreshGit();
    if (state.connected) await loadRemoteRoot();
  } catch (e) {
    setStatus(`init 失敗: ${e}`, "error");
  } finally {
    el.progressWrap.hidden = true;
    updateButtons();
  }
}

// catchup: アップロードせず .git-ftp.log に現在のHEADを記録
async function gitFtpCatchup() {
  closeMenu();
  const prof = currentProfile();
  if (!prof || !state.repoPath) return;

  const ok = window.confirm(
    "【git-ftp catchup】\n\n" +
      `アップロードは行わず、現在のHEAD (${shortHash(state.head)}) を\n` +
      `「${prof.name}」にデプロイ済みとして記録します。\n\n` +
      "既にファイルがサーバー上にある場合に使用します。\n" +
      "以降のアップロードは、このコミット以降の差分のみになります。\n\n" +
      "実行してよろしいですか?"
  );
  if (!ok) {
    setStatus("catchupをキャンセルしました");
    return;
  }

  setStatus("catchup: 状態を記録中…");
  try {
    await invoke("git_ftp_catchup", {
      id: prof.id,
      repoPath: state.repoPath,
    });
    setStatus(`catchup 完了: ${shortHash(state.head)} を記録しました`, "ok");
    await refreshGit();
  } catch (e) {
    setStatus(`catchup 失敗: ${e}`, "error");
  } finally {
    updateButtons();
  }
}

// デプロイ進捗イベント
listen("deploy://progress", (event) => {
  const { current, total, path } = event.payload;
  const pct = total > 0 ? Math.round((current / total) * 100) : 0;
  el.progressBar.style.width = `${pct}%`;
  setStatus(`(${current}/${total}) ${path}`);
});

// ---------- ボタン状態 ----------
function updateButtons() {
  const hasRepo = !!state.repoPath;
  const hasProfile = !!currentProfile();
  const selCount = state.files.filter((f) => f.selected).length;
  el.refreshBtn.disabled = !hasRepo;
  el.connectBtn.disabled = !hasProfile;
  el.editProfileBtn.disabled = !hasProfile;
  el.deployBtn.disabled = !(
    hasRepo &&
    hasProfile &&
    state.connected &&
    selCount > 0
  );
  el.deployBtn.textContent =
    selCount > 0 ? `⇧ アップロード (${selCount})` : "⇧ アップロード";
  // init/catchup は リポジトリ選択 + プロファイル + 接続済み で有効
  el.moreBtn.disabled = !(hasRepo && hasProfile && state.connected);
  if (el.moreBtn.disabled) closeMenu();
}

// ---------- イベント配線 ----------
el.pickRepoBtn.addEventListener("click", pickRepo);
el.refreshBtn.addEventListener("click", refreshGit);
el.connectBtn.addEventListener("click", connectRemote);
el.deployBtn.addEventListener("click", deploy);
el.moreBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  el.moreMenu.hidden = !el.moreMenu.hidden;
});
el.initBtn.addEventListener("click", gitFtpInit);
el.catchupBtn.addEventListener("click", gitFtpCatchup);
// メニュー外クリックで閉じる
document.addEventListener("click", (e) => {
  if (!el.moreMenu.hidden && !e.target.closest(".menu-wrap")) closeMenu();
});
el.addProfileBtn.addEventListener("click", () => openProfileModal(null));
el.editProfileBtn.addEventListener("click", () =>
  openProfileModal(currentProfile())
);
el.profileSelect.addEventListener("change", () => {
  state.currentProfileId = el.profileSelect.value;
  state.connected = false;
  el.remoteStatus.textContent = "未接続";
  el.remoteStatus.style.color = "var(--text-dim)";
  el.remoteTree.innerHTML = `<div class="empty-state"><div class="empty-emoji">🌐</div><p>サーバーに接続してください</p></div>`;
  updateButtons();
});
el.selectAll.addEventListener("change", () => {
  const checked = el.selectAll.checked;
  state.files.forEach((f) => (f.selected = checked));
  renderDiff();
  updateButtons();
});
el.saveProfileBtn.addEventListener("click", saveProfile);
el.cancelProfileBtn.addEventListener("click", () => (el.modal.hidden = true));
el.deleteProfileBtn.addEventListener("click", deleteProfile);
el.f_protocol.addEventListener("change", () => {
  if (!el.f_port.value) el.f_port.value = defaultPort(el.f_protocol.value);
});
el.modal.addEventListener("click", (e) => {
  if (e.target === el.modal) el.modal.hidden = true;
});
// リモートフォルダ選択
el.browseRemoteBtn.addEventListener("click", openPicker);
el.pickerUpBtn.addEventListener("click", pickerUp);
el.pickerSelectBtn.addEventListener("click", pickerSelect);
el.pickerCancelBtn.addEventListener("click", () => (el.pickerModal.hidden = true));
el.pickerModal.addEventListener("click", (e) => {
  if (e.target === el.pickerModal) el.pickerModal.hidden = true;
});

// ---------- 初期化 ----------
(async function init() {
  await loadProfiles();
  setStatus("準備完了");
})();
