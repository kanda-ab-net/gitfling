use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::path::PathBuf;

/// `id` フィールドがキー欠落・null のどちらでも空文字として受け取れるようにする。
fn de_nullable_string<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// サーバー接続プロファイル。パスワードもローカルの設定ファイルに保存する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default, deserialize_with = "de_nullable_string")]
    pub id: String,
    pub name: String,
    /// "ftp" | "ftps" | "sftp"
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub password: String,
    /// このプロファイルに紐づくローカルGitリポジトリの絶対パス。未設定は空文字。
    #[serde(default)]
    pub repo_path: String,
    /// リモート側のデプロイ先ルート（例: /public_html）
    #[serde(default = "default_root")]
    pub remote_root: String,
    /// ローカル側の同期起点（リポジトリルートからの相対パス、例: /wp-content）。
    /// "/" またはリポジトリルートは「リポジトリ全体を同期対象にする」を意味する。
    #[serde(default = "default_root")]
    pub local_root: String,
}

fn default_root() -> String {
    "/".to_string()
}

fn config_path() -> Result<PathBuf, String> {
    let dir = dirs::config_dir()
        .ok_or_else(|| "設定ディレクトリが見つかりません".to_string())?
        .join("gitfling");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("profiles.json"))
}

pub fn load() -> Result<Vec<Profile>, String> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let profiles: Vec<Profile> = serde_json::from_str(&data).unwrap_or_default();
    Ok(profiles)
}

fn persist(profiles: &[Profile]) -> Result<(), String> {
    let path = config_path()?;
    let data = serde_json::to_string_pretty(profiles).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

/// 新規/既存プロファイルを保存し、保存後の（idつき）プロファイルを返す。
pub fn save(mut profile: Profile) -> Result<Profile, String> {
    let mut profiles = load()?;
    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
        profiles.push(profile.clone());
    } else {
        match profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => *existing = profile.clone(),
            None => profiles.push(profile.clone()),
        }
    }
    persist(&profiles)?;
    Ok(profile)
}

pub fn delete(id: &str) -> Result<(), String> {
    let mut profiles = load()?;
    profiles.retain(|p| p.id != id);
    persist(&profiles)?;
    Ok(())
}

pub fn find(id: &str) -> Result<Profile, String> {
    load()?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "プロファイルが見つかりません".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_null_id() {
        // フロントエンドが新規作成時に送る id: null を受け付けられること
        let json = r#"{"id":null,"name":"本番","protocol":"ftp","host":"h","port":21,"user":"u","password":"p","remote_root":"/"}"#;
        let p: Profile = serde_json::from_str(json).expect("null id should deserialize");
        assert_eq!(p.id, "");
        assert_eq!(p.host, "h");
    }

    #[test]
    fn deserializes_missing_id() {
        let json = r#"{"name":"本番","protocol":"sftp","host":"h","port":22,"user":"u","password":"p","remote_root":"/"}"#;
        let p: Profile = serde_json::from_str(json).expect("missing id should deserialize");
        assert_eq!(p.id, "");
    }
}
