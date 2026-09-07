use git2::{Delta, DiffOptions, Oid, Repository, Status, StatusOptions, Tree};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

const IGNORE_FILE: &str = ".git-ftp-ignore";

/// リポジトリルートの .git-ftp-ignore を読み、gitignore互換(glob)のマッチャを作る。
/// ファイルが無い場合は「何もマッチしない」マッチャを返す。
pub fn load_ignore(repo_path: &str) -> Gitignore {
    let root = Path::new(repo_path);
    let mut builder = GitignoreBuilder::new(root);
    if let Ok(content) = std::fs::read_to_string(root.join(IGNORE_FILE)) {
        for line in content.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let _ = builder.add_line(None, l);
        }
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// 指定パスが .git-ftp-ignore により除外されるか。
/// 親ディレクトリのパターン（例: `node_modules/`）も辿って判定する。
pub fn is_ignored(gi: &Gitignore, path: &str) -> bool {
    gi.matched_path_or_any_parents(path, false).is_ignore()
}

#[derive(Debug, Serialize)]
pub struct FileChange {
    pub path: String,
    /// A=追加, M=変更, D=削除, R=改名, U=その他
    pub status: String,
    /// 作業ツリーの内容がHEADのコミット内容と異なる（未コミットの変更を含む）場合 true。
    /// アップロードされる実ファイルがコミット済み内容と食い違うことを示す。
    pub dirty: bool,
}

#[derive(Debug, Serialize)]
pub struct GitStatus {
    pub branch: String,
    pub head: String,
    pub files: Vec<FileChange>,
    /// .git-ftp-ignore により一覧から除外した件数。
    pub ignored: usize,
}

fn status_char(d: Delta) -> &'static str {
    match d {
        Delta::Added | Delta::Copied => "A",
        Delta::Deleted => "D",
        Delta::Modified | Delta::Typechange => "M",
        Delta::Renamed => "R",
        _ => "U",
    }
}

/// ローカル同期ベースフォルダの指定（"/" や空文字はリポジトリルートを意味する）を
/// 「先頭・末尾スラッシュなしの相対パス」に正規化する。結果が空文字ならスコープなし。
fn normalize_local_root(local_root: &str) -> String {
    local_root.trim_matches('/').to_string()
}

/// deployed_hash（サーバーの .git-ftp.log にある最終デプロイコミット）から HEAD までの差分。
/// deployed_hash が None または不正な場合は、HEAD の全追跡ファイルを「追加」として返す（初回デプロイ）。
/// local_root が指定されている場合、そのフォルダ配下のみを対象にし、返すパスはそのフォルダからの相対パスにする。
pub fn status(
    repo_path: &str,
    deployed_hash: Option<String>,
    local_root: &str,
) -> Result<GitStatus, String> {
    let repo = Repository::open(repo_path).map_err(|e| format!("リポジトリを開けません: {e}"))?;

    let head_ref = repo.head().map_err(|e| format!("HEADを取得できません: {e}"))?;
    let branch = head_ref.shorthand().unwrap_or("HEAD").to_string();
    let head_commit = head_ref
        .peel_to_commit()
        .map_err(|e| format!("コミットを取得できません: {e}"))?;
    let head_oid = head_commit.id();
    let head_tree = head_commit.tree().map_err(|e| e.to_string())?;

    // 旧ツリー（前回デプロイ時点）を解決
    let old_tree: Option<Tree> = match deployed_hash {
        Some(h) if !h.is_empty() => Oid::from_str(&h)
            .ok()
            .and_then(|oid| repo.find_commit(oid).ok())
            .and_then(|c| c.tree().ok()),
        _ => None,
    };

    let root = normalize_local_root(local_root);
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };

    let mut opts = DiffOptions::new();
    opts.include_typechange(true);
    if !root.is_empty() {
        opts.pathspec(&root);
    }
    let diff = repo
        .diff_tree_to_tree(old_tree.as_ref(), Some(&head_tree), Some(&mut opts))
        .map_err(|e| format!("差分を計算できません: {e}"))?;

    // 作業ツリーがHEADから変更されているパスの集合（index/worktreeいずれか）
    let dirty = dirty_paths(&repo);
    // .git-ftp-ignore による除外
    let ignore = load_ignore(repo_path);

    let mut files = Vec::new();
    let mut ignored = 0usize;
    for delta in diff.deltas() {
        let status = status_char(delta.status());
        // 削除は old_file、それ以外は new_file のパスを使う
        let path = if status == "D" {
            delta.old_file().path()
        } else {
            delta.new_file().path()
        };
        if let Some(p) = path {
            let full_path = p.to_string_lossy().replace('\\', "/");
            if is_ignored(&ignore, &full_path) {
                ignored += 1;
                continue;
            }
            // local_root 配下のパスのみを対象にし、返すパスはそこからの相対パスにする
            let rel_path = if prefix.is_empty() {
                full_path.clone()
            } else if let Some(stripped) = full_path.strip_prefix(prefix.as_str()) {
                stripped.to_string()
            } else {
                continue;
            };
            // 削除ファイルは実体をアップロードしないので dirty 判定の対象外
            let is_dirty = status != "D" && dirty.contains(&full_path);
            files.push(FileChange {
                path: rel_path,
                status: status.to_string(),
                dirty: is_dirty,
            });
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(GitStatus {
        branch,
        head: head_oid.to_string(),
        files,
        ignored,
    })
}

/// 現在のHEADコミットのハッシュ（40桁）を返す。
pub fn head_hash(repo_path: &str) -> Result<String, String> {
    let repo = Repository::open(repo_path).map_err(|e| format!("リポジトリを開けません: {e}"))?;
    let commit = repo
        .head()
        .and_then(|h| h.peel_to_commit())
        .map_err(|e| format!("HEADを取得できません: {e}"))?;
    Ok(commit.id().to_string())
}

/// HEAD時点の全追跡ファイル（ブロブ）のパス一覧。init（初回全アップロード）で使う。
/// local_root が指定されている場合、そのフォルダ配下のみを対象にし、返すパスはそのフォルダからの相対パスにする。
pub fn tracked_files(repo_path: &str, local_root: &str) -> Result<Vec<String>, String> {
    let repo = Repository::open(repo_path).map_err(|e| format!("リポジトリを開けません: {e}"))?;
    let tree = repo
        .head()
        .and_then(|h| h.peel_to_commit())
        .and_then(|c| c.tree())
        .map_err(|e| format!("HEADツリーを取得できません: {e}"))?;

    // .git-ftp-ignore による除外を初回アップロードでも尊重する
    let ignore = load_ignore(repo_path);
    let root = normalize_local_root(local_root);
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };

    let mut files = Vec::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let name = entry.name().unwrap_or("");
            // dir は "src/" のように末尾スラッシュ付き、ルート直下は ""
            let full_path = format!("{dir}{name}").replace('\\', "/");
            if is_ignored(&ignore, &full_path) {
                return git2::TreeWalkResult::Ok;
            }
            if prefix.is_empty() {
                files.push(full_path);
            } else if let Some(stripped) = full_path.strip_prefix(prefix.as_str()) {
                files.push(stripped.to_string());
            }
        }
        git2::TreeWalkResult::Ok
    })
    .map_err(|e| e.to_string())?;

    files.sort();
    Ok(files)
}

/// ローカルディレクトリのエントリ（サブフォルダのみ）。プロファイルの
/// 「ローカルの同期ベースフォルダ」を選ぶ GUI ダイアログ用。
#[derive(Debug, Serialize)]
pub struct LocalDirEntry {
    pub name: String,
    pub path: String,
}

/// repo_path 配下の path（"/" 起点の相対パス）にあるサブディレクトリ一覧を返す。
pub fn local_browse(repo_path: &str, path: &str) -> Result<Vec<LocalDirEntry>, String> {
    let rel = path.trim_matches('/');
    let dir = if rel.is_empty() {
        Path::new(repo_path).to_path_buf()
    } else {
        Path::new(repo_path).join(rel)
    };
    let read_dir = std::fs::read_dir(&dir).map_err(|e| format!("フォルダを読めません: {e}"))?;

    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let child_path = if rel.is_empty() {
            format!("/{name}")
        } else {
            format!("/{rel}/{name}")
        };
        entries.push(LocalDirEntry { name, path: child_path });
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

/// 作業ツリー/インデックスがHEADと異なる（未コミット変更のある）パス集合を返す。
fn dirty_paths(repo: &Repository) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut sopts = StatusOptions::new();
    sopts
        .include_untracked(true)
        .include_ignored(false)
        .recurse_untracked_dirs(true);
    let changed = Status::INDEX_NEW
        | Status::INDEX_MODIFIED
        | Status::INDEX_DELETED
        | Status::INDEX_RENAMED
        | Status::INDEX_TYPECHANGE
        | Status::WT_NEW
        | Status::WT_MODIFIED
        | Status::WT_DELETED
        | Status::WT_RENAMED
        | Status::WT_TYPECHANGE;
    if let Ok(statuses) = repo.statuses(Some(&mut sopts)) {
        for entry in statuses.iter() {
            if entry.status().intersects(changed) {
                if let Some(p) = entry.path() {
                    set.insert(p.replace('\\', "/"));
                }
            }
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gitfling_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src/lib")).unwrap();
        fs::write(dir.join("index.html"), b"<h1>hi</h1>").unwrap();
        fs::write(dir.join("src/app.js"), b"console.log(1)").unwrap();
        fs::write(dir.join("src/lib/util.js"), b"export const x=1").unwrap();

        let repo = Repository::init(&dir).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Test").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();

        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        dir
    }

    #[test]
    fn tracked_files_includes_nested_paths() {
        let dir = tmp_repo();
        let mut files = tracked_files(dir.to_str().unwrap(), "/").unwrap();
        files.sort();
        assert_eq!(
            files,
            vec![
                "index.html".to_string(),
                "src/app.js".to_string(),
                "src/lib/util.js".to_string(),
            ]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tracked_files_scoped_to_local_root() {
        let dir = tmp_repo();
        let mut files = tracked_files(dir.to_str().unwrap(), "/src").unwrap();
        files.sort();
        assert_eq!(
            files,
            vec!["app.js".to_string(), "lib/util.js".to_string()]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn head_hash_is_40_hex() {
        let dir = tmp_repo();
        let h = head_hash(dir.to_str().unwrap()).unwrap();
        assert_eq!(h.len(), 40);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn git_ftp_ignore_glob_patterns() {
        let dir = tmp_repo();
        fs::write(
            dir.join(".git-ftp-ignore"),
            "# コメント\n*.map\nsrc/**\nREADME.md\nnode_modules/\n",
        )
        .unwrap();
        let gi = load_ignore(dir.to_str().unwrap());

        // 除外される
        assert!(is_ignored(&gi, "app.min.js.map")); // *.map（深さ任意）
        assert!(is_ignored(&gi, "src/app.js")); // src/**
        assert!(is_ignored(&gi, "src/lib/util.js"));
        assert!(is_ignored(&gi, "README.md"));
        assert!(is_ignored(&gi, "node_modules/pkg/index.js")); // 末尾/ ディレクトリ
        // 除外されない
        assert!(!is_ignored(&gi, "index.html"));
        assert!(!is_ignored(&gi, "assets/logo.svg"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_ignore_file_means_nothing_excluded() {
        let dir = tmp_repo();
        let gi = load_ignore(dir.to_str().unwrap());
        assert!(!is_ignored(&gi, "anything.js"));
        fs::remove_dir_all(&dir).ok();
    }
}
