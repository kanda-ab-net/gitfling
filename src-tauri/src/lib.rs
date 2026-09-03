mod deploy;
mod git;
mod profiles;
mod remote;

use deploy::{DeployFile, DeployResult};
use git::GitStatus;
use profiles::Profile;
use remote::RemoteEntry;
use tauri::AppHandle;

// ---------- プロファイル ----------
#[tauri::command]
async fn list_profiles() -> Result<Vec<Profile>, String> {
    tauri::async_runtime::spawn_blocking(profiles::load)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn save_profile(profile: Profile) -> Result<Profile, String> {
    tauri::async_runtime::spawn_blocking(move || profiles::save(profile))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn delete_profile(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || profiles::delete(&id))
        .await
        .map_err(|e| e.to_string())?
}

// ---------- Git ----------
#[tauri::command]
async fn git_status(
    repo_path: String,
    deployed_hash: Option<String>,
) -> Result<GitStatus, String> {
    tauri::async_runtime::spawn_blocking(move || git::status(&repo_path, deployed_hash))
        .await
        .map_err(|e| e.to_string())?
}

// ---------- リモート ----------
#[tauri::command]
async fn remote_connect_test(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profiles::find(&id)?;
        deploy::test_connection(&profile)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn remote_list(id: String, path: String) -> Result<Vec<RemoteEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profiles::find(&id)?;
        let mut conn = remote::connect(&profile)?;
        conn.list(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 入力中（未保存）のプロファイルでその場接続し、指定パス配下の「ディレクトリのみ」を返す。
/// リモートのルートパスを GUI で選ぶ「参照…」ダイアログ用。
#[tauri::command]
async fn remote_browse(profile: Profile, path: String) -> Result<Vec<RemoteEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = remote::connect(&profile)?;
        let mut entries = conn.list(&path)?;
        entries.retain(|e| e.is_dir);
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(entries)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn remote_deployed_hash(id: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profiles::find(&id)?;
        deploy::read_deployed_hash(&profile)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn deploy(
    app: AppHandle,
    id: String,
    repo_path: String,
    files: Vec<DeployFile>,
    head_hash: String,
) -> Result<DeployResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profiles::find(&id)?;
        deploy::deploy(&app, &profile, &repo_path, &files, &head_hash)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// init の確認ダイアログ用に、アップロード対象（HEADの全追跡ファイル）の一覧を返す。
#[tauri::command]
async fn git_tracked_files(repo_path: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || git::tracked_files(&repo_path))
        .await
        .map_err(|e| e.to_string())?
}

/// git ftp init: HEADの全追跡ファイルを初回アップロードし .git-ftp.log を作成。
#[tauri::command]
async fn git_ftp_init(
    app: AppHandle,
    id: String,
    repo_path: String,
) -> Result<DeployResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profiles::find(&id)?;
        let head = git::head_hash(&repo_path)?;
        let files = git::tracked_files(&repo_path)?;
        deploy::init(&app, &profile, &repo_path, &files, &head)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// git ftp catchup: アップロードせず .git-ftp.log に現在のHEADを記録。記録したハッシュを返す。
#[tauri::command]
async fn git_ftp_catchup(id: String, repo_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profiles::find(&id)?;
        let head = git::head_hash(&repo_path)?;
        deploy::catchup(&profile, &head)?;
        Ok(head)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_profiles,
            save_profile,
            delete_profile,
            git_status,
            remote_connect_test,
            remote_list,
            remote_browse,
            remote_deployed_hash,
            deploy,
            git_tracked_files,
            git_ftp_init,
            git_ftp_catchup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
