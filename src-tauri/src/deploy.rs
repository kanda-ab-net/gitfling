use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, Emitter};

use crate::profiles::Profile;
use crate::remote::{self, join_path, parent_dir, Remote};

const LOG_FILE: &str = ".git-ftp.log";

#[derive(Debug, Deserialize)]
pub struct DeployFile {
    pub path: String,
    pub status: String, // A / M / D / R / U
}

#[derive(Debug, Serialize)]
pub struct DeployResult {
    pub uploaded: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    current: usize,
    total: usize,
    path: String,
}

/// サーバー上の .git-ftp.log から最終デプロイのコミットハッシュを読む。
pub fn read_deployed_hash(profile: &Profile) -> Result<Option<String>, String> {
    let mut conn = remote::connect(profile)?;
    let log_path = join_path(&profile.remote_root, LOG_FILE);
    match conn.read_file(&log_path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let hash = text
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .last()
                .map(|s| s.to_string());
            Ok(hash)
        }
        // ファイルが無い＝未デプロイ扱い
        Err(_) => Ok(None),
    }
}

/// 接続確認のみ。
pub fn test_connection(profile: &Profile) -> Result<(), String> {
    let mut conn = remote::connect(profile)?;
    // ルートを一覧できれば成功とみなす
    conn.list(&profile.remote_root)?;
    Ok(())
}

/// ローカルの同期ベースフォルダ（"/" 起点の相対パス）を repo_path に結合した実ディレクトリを返す。
fn local_base(repo_path: &str, local_root: &str) -> std::path::PathBuf {
    let rel = local_root.trim_matches('/');
    if rel.is_empty() {
        Path::new(repo_path).to_path_buf()
    } else {
        Path::new(repo_path).join(rel)
    }
}

/// 選択されたファイルをアップロード / 削除し、最後に .git-ftp.log を更新する。
pub fn deploy(
    app: &AppHandle,
    profile: &Profile,
    repo_path: &str,
    local_root: &str,
    files: &[DeployFile],
    head_hash: &str,
) -> Result<DeployResult, String> {
    let mut conn = remote::connect(profile)?;
    let base = local_base(repo_path, local_root);
    let total = files.len();
    let mut uploaded = 0;
    let mut deleted = 0;

    for (i, f) in files.iter().enumerate() {
        let _ = app.emit(
            "deploy://progress",
            Progress {
                current: i + 1,
                total,
                path: f.path.clone(),
            },
        );

        let remote_path = join_path(&profile.remote_root, &f.path);

        if f.status == "D" {
            // リモートから削除（既に無い場合は無視）
            let _ = conn.delete_file(&remote_path);
            deleted += 1;
        } else {
            // ローカル（作業ツリー）のファイル内容をアップロード
            let local_path = base.join(&f.path);
            let data = std::fs::read(&local_path)
                .map_err(|e| format!("{} を読めません: {e}", f.path))?;
            // 親ディレクトリを用意
            let dir = parent_dir(&remote_path);
            conn.ensure_dir(&dir)?;
            conn.write_file(&remote_path, &data)?;
            uploaded += 1;
        }
    }

    // .git-ftp.log を更新（HEADのハッシュを記録）
    update_log(&mut conn, profile, head_hash)?;

    Ok(DeployResult { uploaded, deleted })
}

/// git ftp init 相当。HEADの全追跡ファイルを初回アップロードし、.git-ftp.log を作成する。
/// 作業ツリーに存在しない（削除済み）追跡ファイルはスキップする。
pub fn init(
    app: &AppHandle,
    profile: &Profile,
    repo_path: &str,
    local_root: &str,
    files: &[String],
    head_hash: &str,
) -> Result<DeployResult, String> {
    let base = local_base(repo_path, local_root);
    let deploy_files: Vec<DeployFile> = files
        .iter()
        .filter(|p| base.join(p).is_file())
        .map(|p| DeployFile {
            path: p.clone(),
            status: "A".to_string(),
        })
        .collect();
    deploy(app, profile, repo_path, local_root, &deploy_files, head_hash)
}

/// git ftp catchup 相当。アップロードは行わず、.git-ftp.log に現在のHEADを記録するだけ。
/// 既に別手段でファイルがサーバー上にある場合に「ここまで反映済み」とマークする。
pub fn catchup(profile: &Profile, head_hash: &str) -> Result<(), String> {
    let mut conn = remote::connect(profile)?;
    update_log(&mut conn, profile, head_hash)?;
    Ok(())
}

fn update_log(
    conn: &mut Box<dyn Remote>,
    profile: &Profile,
    head_hash: &str,
) -> Result<(), String> {
    let log_path = join_path(&profile.remote_root, LOG_FILE);
    conn.ensure_dir(&parent_dir(&log_path))?;
    let content = format!("{head_hash}\n");
    conn.write_file(&log_path, content.as_bytes())?;
    Ok(())
}
