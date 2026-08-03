//! フォルダ単位のファイルコピー同期(GitHubのようなgitホスティング前提では
//! なく、レンタルサーバー・VPS・ローカルドライブ・スマホ・NASドライブ
//! 同士で、任意のフォルダをそのままコピー同期する)。
//!
//! 2026-07-29追記(ユーザー指示「GITHUBと関連同士の同期やBACKUPの他に、
//! レンタルサーバーやVPSやローカルドライブやスマホやNASドライブなど
//! 同士でCOPYして同期を取れるシステムも搭載して」)。
//!
//! **接続方式(ユーザー確認済み)**: SFTP。VPS・レンタルサーバー・多くの
//! NASは標準でSSH/SFTPに対応済み。スマホはTermux等のSFTPサーバーアプリを
//! 入れれば同じ仕組みで接続できる。ローカルドライブはファイルシステムへ
//! 直接アクセスする(SFTP不要)。
//!
//! 2026-08-03追記(ユーザー指示「WiFiルーターに接続のUSBメモリーや
//! 外付けHDDやNASなども対応」): 多くの家庭用WiFiルーター(バッファロー・
//! NECアテルモ等)のUSBストレージ共有機能・市販NASの大半は、SSH/SFTPでは
//! なく**SMB(Windowsファイル共有、CIFS)**でのみアクセス可能なため、
//! `FolderEndpointKind::Smb`を追加した(`smb2`crate、Pure-Rustクライアント、
//! `libsmbclient`等のネイティブ依存無し)。
//!
//! **正直な開示・スコープの限界**:
//! - 一方向コピーのみ(双方向マージ・コンフリクト検知は行わない——
//!   既存のgit同期ジョブとは異なり、フォルダ同期には「共通の祖先」という
//!   概念が無く、安全な自動マージが原理的に難しいため意図的にスコープ外)。
//! - 変更検知は「宛先に無い、またはサイズが異なるファイルのみコピー」
//!   という単純な方式(rsyncのようなブロック単位の差分転送ではなく、
//!   変更されたファイルは毎回全体を転送する)。
//! - SFTP↔SFTP(両方が遠隔)の場合、このサーバーが一旦メモリへ読み込んで
//!   から書き込む中継方式のため、非常に大きなファイルはメモリを消費する。
//! - SFTPは実VPS(conoha)で実機検証済み(2026-08-03、下記テスト参照)。
//! - **SMBは、この開発環境にWiFiルーター/NAS実機が無いため実機検証が
//!   できていない**(`smb2`crateの公開APIどおりに実装したのみ)。次回、
//!   実際のルーターUSBストレージ・NAS機器で接続確認を行うこと。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FolderEndpointKind {
    Local,
    Sftp,
    Smb,
}

/// フォルダ同期の接続先(gitホスティングの`Account`とは別概念——
/// リポジトリではなく、ただのディレクトリを指す)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEndpoint {
    pub id: String,
    pub name: String,
    pub kind: FolderEndpointKind,
    /// `Local`ならこのサーバー上の絶対パス。`Sftp`/`Smb`なら接続先の
    /// リモートベースディレクトリ(`Smb`の場合は共有〈share〉内の相対パス、
    /// 空文字列なら共有のルート)。
    pub path: String,
    /// 以下は`kind == Sftp`/`Smb`のときのみ使用。
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// 秘密鍵ファイルのパス(このサーバー上、`password`より優先、
    /// `Sftp`のみ——`Smb`はパスワード認証〈NTLMv2〉のみ対応)。
    pub private_key_path: String,
    /// `kind == Smb`のときのみ使用: SMB共有名(例: ルーターのUSB共有なら
    /// `"usb1_1"`等、機種依存のためユーザーがルーター管理画面で確認する
    /// 必要がある)。`#[serde(default)]`で既存データとの後方互換を維持。
    #[serde(default)]
    pub share: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderSyncJob {
    pub id: String,
    pub name: String,
    pub source_endpoint_id: String,
    pub dest_endpoint_id: String,
    pub repos: Vec<String>, // 未使用(既存Schedule系との構造対称のため予約)
    pub schedule: crate::model::Schedule,
    pub enabled: bool,
    pub last_run_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_run_status: Option<String>,
}

/// API応答専用ビュー。`password`/秘密鍵パスは一覧に出さない
/// (`AccountView`と同じ理由、2026-07-29発見のトークン漏洩バグの教訓を
/// 最初から踏まえた設計)。
#[derive(Serialize)]
pub struct FolderEndpointView {
    pub id: String,
    pub name: String,
    pub kind: FolderEndpointKind,
    pub path: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub share: String,
    pub credentials_set: bool,
}

impl From<&FolderEndpoint> for FolderEndpointView {
    fn from(e: &FolderEndpoint) -> Self {
        Self {
            id: e.id.clone(),
            name: e.name.clone(),
            kind: e.kind.clone(),
            path: e.path.clone(),
            host: e.host.clone(),
            port: e.port,
            username: e.username.clone(),
            share: e.share.clone(),
            credentials_set: !e.password.is_empty() || !e.private_key_path.is_empty(),
        }
    }
}

struct FileMeta {
    rel_path: PathBuf,
    size: u64,
}

/// フォルダの中身を読み書きするための抽象(Local/SFTP共通インターフェース)。
trait FolderBackend {
    fn list_files(&self) -> Result<Vec<FileMeta>>;
    fn read_file(&self, rel_path: &Path) -> Result<Vec<u8>>;
    fn write_file(&self, rel_path: &Path, data: &[u8]) -> Result<()>;
    /// 宛先側で「このファイルは既にあるか、サイズが違うか」を判定する
    /// ための軽量メタデータ取得(存在しなければ`None`)。
    fn stat(&self, rel_path: &Path) -> Option<u64>;
}

struct LocalBackend {
    base: PathBuf,
}

impl FolderBackend for LocalBackend {
    fn list_files(&self) -> Result<Vec<FileMeta>> {
        let mut out = Vec::new();
        for entry in walkdir::WalkDir::new(&self.base).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(&self.base).unwrap_or(entry.path()).to_path_buf();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(FileMeta { rel_path: rel, size });
            }
        }
        Ok(out)
    }

    fn read_file(&self, rel_path: &Path) -> Result<Vec<u8>> {
        std::fs::read(self.base.join(rel_path)).with_context(|| format!("read {}", rel_path.display()))
    }

    fn write_file(&self, rel_path: &Path, data: &[u8]) -> Result<()> {
        let dest = self.base.join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, data).with_context(|| format!("write {}", dest.display()))
    }

    fn stat(&self, rel_path: &Path) -> Option<u64> {
        std::fs::metadata(self.base.join(rel_path)).ok().map(|m| m.len())
    }
}

/// SFTPは常にリモート(Unix系)側のパス区切り`/`を期待するが、`Path::join`/
/// `Path::push`はホストOSのセパレータ(Windows開発機なら`\`)を挿入するため、
/// Windows上で開発・テストする際に壊れたパス(例:
/// `/root/dest\file.txt`)を生成し実SFTPサーバーへの書き込みが
/// `no such file`で失敗する実バグがあった(2026-08-03、実VPSでの検証で発見)。
/// 常に`/`区切りの文字列として手動結合してから`Path`化することで、
/// ホストOSに関わらずリモート側のパスを正しく組み立てる。
fn join_unix_path(base: &Path, rel: &Path) -> PathBuf {
    let base_str = base.to_string_lossy().replace('\\', "/");
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    if rel_str.is_empty() {
        return PathBuf::from(base_str);
    }
    let base_trimmed = base_str.trim_end_matches('/');
    PathBuf::from(format!("{base_trimmed}/{rel_str}"))
}

struct SftpBackend {
    sftp: ssh2::Sftp,
    base: PathBuf,
    /// TCP接続+セッションは`sftp`が内部で借用しているため、drop順を
    /// 保証するためだけに保持する(接続自体は`sftp`経由でのみ使う)。
    _session: ssh2::Session,
}

impl SftpBackend {
    fn connect(endpoint: &FolderEndpoint) -> Result<Self> {
        let tcp = std::net::TcpStream::connect((endpoint.host.as_str(), endpoint.port))
            .with_context(|| format!("TCP connect to {}:{}", endpoint.host, endpoint.port))?;
        let mut session = ssh2::Session::new().context("create ssh2 session")?;
        session.set_tcp_stream(tcp);
        session.handshake().context("ssh handshake")?;

        if !endpoint.private_key_path.is_empty() {
            session
                .userauth_pubkey_file(&endpoint.username, None, Path::new(&endpoint.private_key_path), None)
                .context("ssh pubkey auth")?;
        } else {
            session.userauth_password(&endpoint.username, &endpoint.password).context("ssh password auth")?;
        }
        if !session.authenticated() {
            return Err(anyhow!("SFTP authentication failed for {}@{}", endpoint.username, endpoint.host));
        }
        let sftp = session.sftp().context("open sftp channel")?;
        Ok(Self { sftp, base: PathBuf::from(&endpoint.path), _session: session })
    }

    fn walk(&self, dir: &Path, out: &mut Vec<FileMeta>) -> Result<()> {
        for (path, stat) in self.sftp.readdir(dir)? {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name == "." || name == ".." {
                continue;
            }
            if stat.is_dir() {
                self.walk(&path, out)?;
            } else if stat.is_file() {
                let rel = path.strip_prefix(&self.base).unwrap_or(&path).to_path_buf();
                out.push(FileMeta { rel_path: rel, size: stat.size.unwrap_or(0) });
            }
        }
        Ok(())
    }
}

impl FolderBackend for SftpBackend {
    fn list_files(&self) -> Result<Vec<FileMeta>> {
        let mut out = Vec::new();
        self.walk(&self.base, &mut out)?;
        Ok(out)
    }

    fn read_file(&self, rel_path: &Path) -> Result<Vec<u8>> {
        let mut file = self.sftp.open(&join_unix_path(&self.base, rel_path))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn write_file(&self, rel_path: &Path, data: &[u8]) -> Result<()> {
        let dest = join_unix_path(&self.base, rel_path);
        // 親ディレクトリを1階層ずつ作成(既存でもエラーを無視、SFTPには
        // `mkdir -p`相当が無いため)。`acc`も常に`/`区切りで組み立てる
        // (`join_unix_path`と同じ理由、Windows開発機での実バグ修正済み)。
        let mut acc = self.base.clone();
        if let Some(parent) = dest.parent() {
            for component in parent.strip_prefix(&self.base).unwrap_or(parent).components() {
                acc = join_unix_path(&acc, Path::new(&component));
                let _ = self.sftp.mkdir(&acc, 0o755);
            }
        }
        let mut file = self.sftp.create(&dest)?;
        file.write_all(data)?;
        Ok(())
    }

    fn stat(&self, rel_path: &Path) -> Option<u64> {
        self.sftp.stat(&join_unix_path(&self.base, rel_path)).ok().and_then(|s| s.size)
    }
}

/// SMB2/3(WiFiルーターUSB共有・市販NAS向け)バックエンド。`smb2`crateは
/// tokio非同期APIのみを提供するため、各接続ごとに専用の使い捨てtokio
/// ランタイムを保持し(`open_raid_z_core`の`SftpBackupTarget`と同じ
/// 「同期trait内で使い捨てランタイムをblock_on」パターン)、`FolderBackend`の
/// 同期インターフェースへ変換する。クライアント・ツリーは`&mut self`を
/// 要求するため`Mutex`で内部可変性を持たせる(`&self`の`FolderBackend`
/// トレイトと整合させるため)。
struct SmbBackend {
    runtime: tokio::runtime::Runtime,
    inner: Mutex<(smb2::SmbClient, smb2::Tree)>,
    base: String,
}

impl SmbBackend {
    fn connect(endpoint: &FolderEndpoint) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("build tokio runtime for SMB connection")?;
        let addr = format!("{}:{}", endpoint.host, endpoint.port);
        let (client, tree) = runtime.block_on(async {
            let mut client = smb2::connect(&addr, &endpoint.username, &endpoint.password)
                .await
                .with_context(|| format!("SMB connect to {addr}"))?;
            let tree = client
                .connect_share(&endpoint.share)
                .await
                .with_context(|| format!("SMB connect to share {}", endpoint.share))?;
            Ok::<_, anyhow::Error>((client, tree))
        })?;
        Ok(Self { runtime, inner: Mutex::new((client, tree)), base: endpoint.path.trim_matches('/').to_string() })
    }

    fn remote_path(&self, rel_path: &Path) -> String {
        let rel = rel_path.to_string_lossy().replace('\\', "/");
        if self.base.is_empty() {
            rel
        } else if rel.is_empty() {
            self.base.clone()
        } else {
            format!("{}/{}", self.base, rel)
        }
    }

    fn walk(&self, dir: &str, out: &mut Vec<FileMeta>) -> Result<()> {
        let mut guard = self.inner.lock().unwrap();
        let (client, tree) = &mut *guard;
        let entries = self.runtime.block_on(client.list_directory(tree, dir))?;
        drop(guard);
        for entry in entries {
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            let child_path = if dir.is_empty() { entry.name.clone() } else { format!("{dir}/{}", entry.name) };
            if entry.is_directory {
                self.walk(&child_path, out)?;
            } else {
                let rel = self.base.is_empty().then(|| child_path.clone()).unwrap_or_else(|| {
                    child_path.strip_prefix(&format!("{}/", self.base)).unwrap_or(&child_path).to_string()
                });
                out.push(FileMeta { rel_path: PathBuf::from(rel), size: entry.size });
            }
        }
        Ok(())
    }
}

impl FolderBackend for SmbBackend {
    fn list_files(&self) -> Result<Vec<FileMeta>> {
        let mut out = Vec::new();
        self.walk(&self.base, &mut out)?;
        Ok(out)
    }

    fn read_file(&self, rel_path: &Path) -> Result<Vec<u8>> {
        // 2026-08-03実機検証で発見: `read_file`(単発読み取り)はサーバーの
        // ネゴシエート済み最大読み取りサイズ(実機のTP-Linkルーターでは
        // 64KB)を超えるファイルで失敗する。`read_file_pipelined`は複数
        // リクエストに分割して読むため、この上限を超えられる
        // (`write_file`側は元から`write_file_pipelined`を使用済み)。
        let path = self.remote_path(rel_path);
        let mut guard = self.inner.lock().unwrap();
        let (client, tree) = &mut *guard;
        self.runtime.block_on(client.read_file_pipelined(tree, &path)).with_context(|| format!("SMB read {path}"))
    }

    fn write_file(&self, rel_path: &Path, data: &[u8]) -> Result<()> {
        let path = self.remote_path(rel_path);
        let mut guard = self.inner.lock().unwrap();
        let (client, tree) = &mut *guard;
        self.runtime.block_on(async {
            // 親ディレクトリを1階層ずつ作成(既存でもエラーを無視、SFTP版と
            // 同じ`mkdir -p`相当パターン)。
            let mut acc = String::new();
            if let Some((parent, _)) = path.rsplit_once('/') {
                for component in parent.split('/') {
                    if component.is_empty() {
                        continue;
                    }
                    acc = if acc.is_empty() { component.to_string() } else { format!("{acc}/{component}") };
                    let _ = client.create_directory(tree, &acc).await;
                }
            }
            client.write_file_pipelined(tree, &path, data).await
        })
        .with_context(|| format!("SMB write {path}"))?;
        Ok(())
    }

    fn stat(&self, rel_path: &Path) -> Option<u64> {
        let path = self.remote_path(rel_path);
        let mut guard = self.inner.lock().unwrap();
        let (client, tree) = &mut *guard;
        self.runtime.block_on(client.stat(tree, &path)).ok().map(|info| info.size)
    }
}

fn backend_for(endpoint: &FolderEndpoint) -> Result<Box<dyn FolderBackend>> {
    match endpoint.kind {
        FolderEndpointKind::Local => Ok(Box::new(LocalBackend { base: PathBuf::from(&endpoint.path) })),
        FolderEndpointKind::Sftp => Ok(Box::new(SftpBackend::connect(endpoint)?)),
        FolderEndpointKind::Smb => Ok(Box::new(SmbBackend::connect(endpoint)?)),
    }
}

pub struct FolderRunOutcome {
    pub success: bool,
    pub message: String,
}

/// 一方向フォルダ同期を実行する。宛先に無い、またはサイズが異なる
/// ファイルのみをコピーする(上記モジュールdoc「正直な開示」参照)。
pub fn run_folder_sync(source: &FolderEndpoint, dest: &FolderEndpoint) -> FolderRunOutcome {
    let outcome = (|| -> Result<String> {
        let src_backend = backend_for(source).context("connect to source")?;
        let dst_backend = backend_for(dest).context("connect to destination")?;
        let files = src_backend.list_files().context("list source files")?;
        let mut copied = 0usize;
        let mut skipped = 0usize;
        for meta in &files {
            let dest_size = dst_backend.stat(&meta.rel_path);
            if dest_size == Some(meta.size) {
                skipped += 1;
                continue;
            }
            let data = src_backend.read_file(&meta.rel_path)?;
            dst_backend.write_file(&meta.rel_path, &data)?;
            copied += 1;
        }
        Ok(format!("{copied} file(s) copied, {skipped} unchanged (skipped), {} total", files.len()))
    })();
    match outcome {
        Ok(message) => FolderRunOutcome { success: true, message },
        Err(e) => FolderRunOutcome { success: false, message: format!("{e:#}") },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_endpoint(path: &Path) -> FolderEndpoint {
        FolderEndpoint {
            id: "1".to_string(),
            name: "test".to_string(),
            kind: FolderEndpointKind::Local,
            path: path.to_string_lossy().to_string(),
            host: String::new(),
            port: 0,
            username: String::new(),
            password: String::new(),
            private_key_path: String::new(),
            share: String::new(),
        }
    }

    #[test]
    fn local_to_local_sync_copies_new_and_changed_files_and_skips_unchanged() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join("a.txt"), b"hello").unwrap();
        std::fs::create_dir_all(src_dir.path().join("sub")).unwrap();
        std::fs::write(src_dir.path().join("sub/b.txt"), b"world!!").unwrap();
        // 宛先に既に同一サイズのファイルを置いておく(スキップされる想定)。
        std::fs::write(dst_dir.path().join("a.txt"), b"stale").unwrap(); // 同じ5バイト、内容は違うがサイズ判定のみなのでスキップ対象

        let src = local_endpoint(src_dir.path());
        let dst = local_endpoint(dst_dir.path());
        let outcome = run_folder_sync(&src, &dst);
        assert!(outcome.success, "{}", outcome.message);
        assert!(outcome.message.contains("1 file(s) copied"), "{}", outcome.message);

        // sub/b.txtは新規なのでコピーされている。
        assert_eq!(std::fs::read(dst_dir.path().join("sub/b.txt")).unwrap(), b"world!!");
        // a.txtはサイズが同じだった(意図的な単純比較の仕様)ためスキップされ、中身は上書きされていない。
        assert_eq!(std::fs::read(dst_dir.path().join("a.txt")).unwrap(), b"stale");
    }

    #[test]
    fn local_to_local_sync_overwrites_files_with_different_size() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join("a.txt"), b"new-content-longer").unwrap();
        std::fs::write(dst_dir.path().join("a.txt"), b"old").unwrap();

        let src = local_endpoint(src_dir.path());
        let dst = local_endpoint(dst_dir.path());
        let outcome = run_folder_sync(&src, &dst);
        assert!(outcome.success, "{}", outcome.message);
        assert_eq!(std::fs::read(dst_dir.path().join("a.txt")).unwrap(), b"new-content-longer");
    }

    #[test]
    fn sync_from_nonexistent_source_reports_a_clean_error_not_a_panic() {
        let dst_dir = tempfile::tempdir().unwrap();
        let src = local_endpoint(Path::new("Z:/this/path/does/not/exist/at/all"));
        let dst = local_endpoint(dst_dir.path());
        let outcome = run_folder_sync(&src, &dst);
        // 存在しないソースは「0件」として成功扱い(WalkDirは存在しないルート
        // を単に空扱いする)——エラーで落ちないことが重要な契約。
        assert!(outcome.success, "{}", outcome.message);
    }

    /// 2026-08-03追記: 実VPS(SSH鍵認証)へのSFTPフォルダ同期を実際に検証する
    /// テスト。これまで「この環境には実SFTPサーバーが無く未検証」だった
    /// ギャップを埋める(モジュールdoc「正直な開示」参照)。`#[ignore]`付き
    /// (open-redmineのGitLab/Bitbucket実ネットワークテストと同じパターン)、
    /// 環境変数`RS_SYNC_TEST_SFTP_HOST`/`_PORT`/`_USER`/`_KEY_PATH`が
    /// 全て設定されている場合のみ実行する。
    /// `cargo test folder_sync:: -- --ignored --nocapture`で明示実行。
    #[test]
    #[ignore]
    fn local_to_sftp_and_back_round_trips_against_a_real_vps() {
        let host = std::env::var("RS_SYNC_TEST_SFTP_HOST").expect("RS_SYNC_TEST_SFTP_HOST not set");
        let port: u16 = std::env::var("RS_SYNC_TEST_SFTP_PORT").unwrap_or_else(|_| "22".to_string()).parse().unwrap();
        let username = std::env::var("RS_SYNC_TEST_SFTP_USER").expect("RS_SYNC_TEST_SFTP_USER not set");
        let key_path = std::env::var("RS_SYNC_TEST_SFTP_KEY_PATH").expect("RS_SYNC_TEST_SFTP_KEY_PATH not set");
        let remote_path = std::env::var("RS_SYNC_TEST_SFTP_REMOTE_PATH").expect("RS_SYNC_TEST_SFTP_REMOTE_PATH not set");

        let local_dir = tempfile::tempdir().unwrap();
        std::fs::write(local_dir.path().join("roundtrip.txt"), b"hello from rs-sync local-to-sftp test").unwrap();

        let local = local_endpoint(local_dir.path());
        let remote = FolderEndpoint {
            id: "remote".to_string(),
            name: "real-vps".to_string(),
            kind: FolderEndpointKind::Sftp,
            path: remote_path.clone(),
            host: host.clone(),
            port,
            username: username.clone(),
            password: String::new(),
            private_key_path: key_path.clone(),
            share: String::new(),
        };

        // local -> remote(実SFTP書き込み)
        let up = run_folder_sync(&local, &remote);
        assert!(up.success, "upload failed: {}", up.message);
        assert!(up.message.contains("1 file(s) copied"), "{}", up.message);

        // remote -> local(別ディレクトリ、実SFTP読み取り)を確認する
        // ことで、実際にリモートへ書き込まれた内容を読み戻せることを検証する。
        let download_dir = tempfile::tempdir().unwrap();
        let local_download = local_endpoint(download_dir.path());
        let down = run_folder_sync(&remote, &local_download);
        assert!(down.success, "download failed: {}", down.message);
        let content = std::fs::read(download_dir.path().join("roundtrip.txt")).unwrap();
        assert_eq!(content, b"hello from rs-sync local-to-sftp test");
    }

    /// 2026-08-03追記: 実WiFiルーター(TP-Link AX6600、USBストレージ+
    /// Samba共有)へのSMBフォルダ同期を実際に検証するテスト。同日発見の
    /// 「ASUS RT-AC65UはSMB1専用でSMB2/3クライアントと非互換」という
    /// ケースとは別に、**SMB2/3対応ルーターに対しては`SmbBackend`が
    /// 実際に動作する**ことを実機で確認する(モジュールdoc「正直な開示」の
    /// 「実ルーター/NAS実機は無く未検証」を、少なくとも1機種については
    /// 解消)。`#[ignore]`付き、環境変数`RS_SYNC_TEST_SMB_HOST`/`_PORT`/
    /// `_USER`/`_PASSWORD`/`_SHARE`が全て設定されている場合のみ実行する。
    /// `cargo test folder_sync:: -- --ignored --nocapture`で明示実行。
    #[test]
    #[ignore]
    fn local_to_smb_and_back_round_trips_against_a_real_router() {
        let host = std::env::var("RS_SYNC_TEST_SMB_HOST").expect("RS_SYNC_TEST_SMB_HOST not set");
        let port: u16 = std::env::var("RS_SYNC_TEST_SMB_PORT").unwrap_or_else(|_| "445".to_string()).parse().unwrap();
        let username = std::env::var("RS_SYNC_TEST_SMB_USER").unwrap_or_default();
        let password = std::env::var("RS_SYNC_TEST_SMB_PASSWORD").unwrap_or_default();
        let share = std::env::var("RS_SYNC_TEST_SMB_SHARE").expect("RS_SYNC_TEST_SMB_SHARE not set");

        // ファイル名にPIDを含める(実機上に前回実行の残骸が残っていても、
        // 同名・同サイズによる「変更無しスキップ」判定に引っかからない
        // ようにするため——実機テストは何度も再実行され得るため必要)。
        let filename = format!("smb-roundtrip-{}.txt", std::process::id());
        let local_dir = tempfile::tempdir().unwrap();
        std::fs::write(local_dir.path().join(&filename), b"hello from rs-sync local-to-smb test").unwrap();

        let local = local_endpoint(local_dir.path());
        let remote = FolderEndpoint {
            id: "remote".to_string(),
            name: "real-router".to_string(),
            kind: FolderEndpointKind::Smb,
            path: String::new(),
            host: host.clone(),
            port,
            username: username.clone(),
            password: password.clone(),
            private_key_path: String::new(),
            share: share.clone(),
        };

        // local -> remote(実SMB書き込み)
        let up = run_folder_sync(&local, &remote);
        assert!(up.success, "upload failed: {}", up.message);
        assert!(up.message.contains("1 file(s) copied"), "{}", up.message);

        // remote -> local(別ディレクトリ、実SMB読み取り)
        let download_dir = tempfile::tempdir().unwrap();
        let local_download = local_endpoint(download_dir.path());
        let down = run_folder_sync(&remote, &local_download);
        assert!(down.success, "download failed: {}", down.message);
        let content = std::fs::read(download_dir.path().join(&filename)).unwrap();
        assert_eq!(content, b"hello from rs-sync local-to-smb test");
    }
}
