use serde::Serialize;
use std::io::Read;
use std::net::TcpStream;
use std::path::Path;

use crate::profiles::Profile;

#[derive(Debug, Serialize)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// FTP/FTPS/SFTP を共通に扱うための抽象。
pub trait Remote {
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>, String>;
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String>;
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String>;
    fn delete_file(&mut self, path: &str) -> Result<(), String>;
    /// path までの各階層ディレクトリを（無ければ）作成する。
    fn ensure_dir(&mut self, path: &str) -> Result<(), String>;
}

/// POSIX 風のパス結合（末尾/先頭スラッシュを正規化）。
pub fn join_path(base: &str, rel: &str) -> String {
    let base = base.trim_end_matches('/');
    let rel = rel.trim_start_matches('/');
    if base.is_empty() {
        format!("/{rel}")
    } else {
        format!("{base}/{rel}")
    }
}

/// path の親ディレクトリを返す（"/a/b/c.txt" -> "/a/b"）。
pub fn parent_dir(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(idx) => path[..idx].to_string(),
    }
}

/// プロファイルから接続を確立する。
pub fn connect(profile: &Profile) -> Result<Box<dyn Remote>, String> {
    match profile.protocol.as_str() {
        "ftp" | "ftps" => {
            let ftp = FtpRemote::connect(profile)?;
            Ok(Box::new(ftp))
        }
        "sftp" => {
            let sftp = SftpRemote::connect(profile)?;
            Ok(Box::new(sftp))
        }
        other => Err(format!("未対応のプロトコル: {other}")),
    }
}

// ============================================================
// FTP / FTPS
// ============================================================
use suppaftp::list::File as FtpListFile;
use suppaftp::native_tls::TlsConnector;
use suppaftp::{NativeTlsConnector, NativeTlsFtpStream};

pub struct FtpRemote {
    stream: NativeTlsFtpStream,
}

impl FtpRemote {
    fn connect(profile: &Profile) -> Result<Self, String> {
        let addr = format!("{}:{}", profile.host, profile.port);
        let mut stream = NativeTlsFtpStream::connect(&addr)
            .map_err(|e| format!("FTP接続に失敗: {e}"))?;

        // FTPS（明示的TLS）の場合はセキュア接続へ昇格
        if profile.protocol == "ftps" {
            let connector = TlsConnector::new().map_err(|e| e.to_string())?;
            stream = stream
                .into_secure(NativeTlsConnector::from(connector), &profile.host)
                .map_err(|e| format!("TLS昇格に失敗: {e}"))?;
        }

        stream
            .login(&profile.user, &profile.password)
            .map_err(|e| format!("ログインに失敗: {e}"))?;

        // 転送はバイナリモードで
        stream
            .transfer_type(suppaftp::types::FileType::Binary)
            .map_err(|e| e.to_string())?;

        Ok(FtpRemote { stream })
    }
}

impl Remote for FtpRemote {
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>, String> {
        let lines = self
            .stream
            .list(Some(path))
            .map_err(|e| format!("一覧取得に失敗: {e}"))?;
        let mut entries = Vec::new();
        for line in lines {
            if let Ok(f) = FtpListFile::try_from(line.as_str()) {
                let name = f.name().to_string();
                if name == "." || name == ".." {
                    continue;
                }
                entries.push(RemoteEntry {
                    path: join_path(path, &name),
                    name,
                    is_dir: f.is_directory(),
                    size: f.size() as u64,
                });
            }
        }
        Ok(entries)
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let cursor = self
            .stream
            .retr_as_buffer(path)
            .map_err(|e| format!("ダウンロードに失敗: {e}"))?;
        Ok(cursor.into_inner())
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
        let mut reader = std::io::Cursor::new(data);
        self.stream
            .put_file(path, &mut reader)
            .map_err(|e| format!("アップロードに失敗: {e}"))?;
        Ok(())
    }

    fn delete_file(&mut self, path: &str) -> Result<(), String> {
        self.stream
            .rm(path)
            .map_err(|e| format!("削除に失敗: {e}"))?;
        Ok(())
    }

    fn ensure_dir(&mut self, path: &str) -> Result<(), String> {
        let path = path.trim_end_matches('/');
        if path.is_empty() || path == "/" {
            return Ok(());
        }
        // ルートから順に mkdir（既存なら無視）
        let mut cur = String::new();
        for seg in path.split('/') {
            if seg.is_empty() {
                continue;
            }
            cur.push('/');
            cur.push_str(seg);
            // 既に存在する場合はエラーになるので無視する
            let _ = self.stream.mkdir(&cur);
        }
        Ok(())
    }
}

// ============================================================
// SFTP (SSH)
// ============================================================
use ssh2::Session;

pub struct SftpRemote {
    session: Session,
}

impl SftpRemote {
    fn connect(profile: &Profile) -> Result<Self, String> {
        let addr = format!("{}:{}", profile.host, profile.port);
        let tcp = TcpStream::connect(&addr).map_err(|e| format!("接続に失敗: {e}"))?;
        let mut session = Session::new().map_err(|e| e.to_string())?;
        session.set_tcp_stream(tcp);
        session
            .handshake()
            .map_err(|e| format!("SSHハンドシェイクに失敗: {e}"))?;
        session
            .userauth_password(&profile.user, &profile.password)
            .map_err(|e| format!("認証に失敗: {e}"))?;
        if !session.authenticated() {
            return Err("認証に失敗しました".to_string());
        }
        Ok(SftpRemote { session })
    }
}

impl Remote for SftpRemote {
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>, String> {
        let sftp = self.session.sftp().map_err(|e| e.to_string())?;
        let items = sftp
            .readdir(Path::new(path))
            .map_err(|e| format!("一覧取得に失敗: {e}"))?;
        let mut entries = Vec::new();
        for (p, stat) in items {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name == "." || name == ".." || name.is_empty() {
                continue;
            }
            entries.push(RemoteEntry {
                path: join_path(path, &name),
                name,
                is_dir: stat.is_dir(),
                size: stat.size.unwrap_or(0),
            });
        }
        Ok(entries)
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let sftp = self.session.sftp().map_err(|e| e.to_string())?;
        let mut file = sftp
            .open(Path::new(path))
            .map_err(|e| format!("ダウンロードに失敗: {e}"))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
        use std::io::Write;
        let sftp = self.session.sftp().map_err(|e| e.to_string())?;
        let mut file = sftp
            .create(Path::new(path))
            .map_err(|e| format!("アップロードに失敗: {e}"))?;
        file.write_all(data).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn delete_file(&mut self, path: &str) -> Result<(), String> {
        let sftp = self.session.sftp().map_err(|e| e.to_string())?;
        sftp.unlink(Path::new(path))
            .map_err(|e| format!("削除に失敗: {e}"))?;
        Ok(())
    }

    fn ensure_dir(&mut self, path: &str) -> Result<(), String> {
        let sftp = self.session.sftp().map_err(|e| e.to_string())?;
        let path = path.trim_end_matches('/');
        if path.is_empty() || path == "/" {
            return Ok(());
        }
        let mut cur = String::new();
        for seg in path.split('/') {
            if seg.is_empty() {
                continue;
            }
            cur.push('/');
            cur.push_str(seg);
            // 既存なら失敗するので無視
            let _ = sftp.mkdir(Path::new(&cur), 0o755);
        }
        Ok(())
    }
}
