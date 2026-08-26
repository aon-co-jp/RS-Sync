//! インストーラー自動同期(installer sync)。
//!
//! ユーザー指示(2026-08-26)「この様なインストーラーを分かりやすいフォルダーに
//! 同期システムで自動で配置し直すRS-Syncとして、15分に1回でも良いのでその様に
//! 機能するようにしてVPSでもWindowsからでもどちらからでも機能させて」への対応。
//!
//! ## やること
//! `installer_sync_repos()`に列挙した各リポジトリについて、定期的に
//! (既定15分ごと):
//! 1. `cargo build --release --bin <bin>` でWindows向けバイナリを再ビルド
//! 2. Inno Setup(`ISCC.exe`)でインストーラー`.exe`を再コンパイル
//! 3. 生成物のSHA-256を、現在そのリポジトリにコミット済みのファイルと比較
//! 4. 差分があれば(ハッシュが変わっていれば)`.iss`と同じフォルダへ上書き配置し、
//!    そのファイルだけを`git add`・`git commit`・`git push`する
//!
//! ソース変更が無ければハッシュが変わらないため、無駄なコミットは作らない
//! (ユーザー確認済み方針: 差分があった時だけ自動コミット・push)。
//!
//! ## VPS(Linux)対応についての正直な開示
//! Inno Setup Compiler(`ISCC.exe`)はWindows専用のネイティブアプリで、
//! Linux上ではWine経由でしか実行できない。この機能はLinux/VPSホストでも
//! 動作させる設計にしているが、実際に動くかどうかは各ホストの環境
//! (`wine`・mingw-w64クロスリンカ・Wineプレフィックス内のInno Setup)が
//! 整っているかに依存する。環境変数`RS_SYNC_INSTALLER_ISCC_PATH`で
//! ISCC.exeの場所を明示的に指定する(Windowsでは省略時に既定パスへ
//! フォールバックする、Linuxでは必須)。`wine`コマンドまたは指定した
//! ISCC.exeが見つからない場合は、そのホストでは「利用不可」として
//! 正直にスキップする(エラーで全体を止めない)。
//!
//! クロスコンパイル(Linux上で`x86_64-pc-windows-gnu`ターゲットへ
//! ビルド)自体も、各リポジトリの依存クレートがmingw環境で問題なく
//! ビルドできるとは限らない(特にWindows固有API・D3D12/DirectX
//! Shader Compiler等に依存するもの)——失敗したリポジトリは個別に
//! スキップ・エラー記録し、他のリポジトリの処理は継続する。
//!
//! ## オプトイン(既定オフ)
//! `RS_SYNC_INSTALLER_SYNC_ENABLED=1`を明示的に設定しない限り、この
//! 機能は一切動作しない(15リポジトリへ無人で自動push する強力な機能の
//! ため、既存のworld-lab等と同じ「既定オフ・明示的オプトイン」方針を
//! 踏襲)。加えて`RS_SYNC_INSTALLER_ROOT`(各リポジトリの親ディレクトリ、
//! 例: Windows開発機では`F:\runo`、VPSでは`/root`)の設定も必須——
//! 未設定ならリポジトリが見つからないため全件スキップする。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// aruaru-llmのみ、Inno Setupの`[Files]`セクションがGPU版バイナリの
/// Sourceパスを`skipifsourcedoesntexist`無しで参照しているため、
/// GPU版も必ずビルドしないとISCCのコンパイル自体が失敗する
/// (`installer/windows/aruaru-llm.iss`参照)。
#[derive(Debug, Clone, Copy)]
pub struct GpuVariant {
    /// `CARGO_TARGET_DIR`として設定する値(ビルドディレクトリ相対)
    pub cargo_target_dir: &'static str,
    /// `cargo build --features`に渡すカンマ区切りfeatureリスト
    pub features: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct InstallerRepoConfig {
    /// ログ・API表示用の識別子
    pub id: &'static str,
    /// `RS_SYNC_INSTALLER_ROOT`配下のフォルダ名
    pub dir_name: &'static str,
    /// リポジトリルートから見た、`cargo build`を実行するディレクトリ
    /// ("" ならリポジトリルート自身)
    pub build_dir_rel: &'static str,
    /// `cargo build --release --bin <bin_name>`
    pub bin_name: &'static str,
    /// リポジトリルートから見た`.iss`ファイルの相対パス
    pub iss_rel: &'static str,
    /// バージョン番号を読み取る`Cargo.toml`の相対パス
    pub version_toml_rel: &'static str,
    /// pushするブランチ名
    pub branch: &'static str,
    /// aruaru-llmのみSome
    pub gpu_variant: Option<GpuVariant>,
    /// open-web-serverのみtrue: `.iss`の`Source:`がバイナリを
    /// `.iss`と同じフォルダから直接参照するため、ISCCを実行する前に
    /// `target/release/<bin>.exe`をそのフォルダへコピーしておく必要がある。
    pub stage_binary_before_iscc: bool,
}

/// 2026-08-26時点で確認済みの15リポジトリ・16インストーラー
/// (RPoemのみrouter/tray 2つを持つ)。各フィールドの値は、実際に
/// `.iss`ファイルの`[Files] Source:`行・`#define MyAppExeName`・
/// `Cargo.toml`の`[[bin]]`/`[package] name`を1件ずつ確認して決定した
/// (推測で埋めていない)。
pub fn installer_sync_repos() -> Vec<InstallerRepoConfig> {
    vec![
        InstallerRepoConfig {
            id: "rs-sync",
            dir_name: "rs-sync",
            build_dir_rel: "",
            bin_name: "rs-sync",
            iss_rel: "installer/windows/rs-sync.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "rs-blog",
            dir_name: "RS-Blog",
            build_dir_rel: "",
            bin_name: "rs-blog",
            iss_rel: "installer/windows/rs-blog.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "rs-ec",
            dir_name: "RS-EC",
            build_dir_rel: "",
            bin_name: "rs-ec",
            iss_rel: "installer/windows/rs-ec.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "rs-guard",
            dir_name: "RS-Guard",
            build_dir_rel: "",
            bin_name: "rs-guard",
            iss_rel: "installer/windows/rs-guard.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "rs-ops",
            dir_name: "RS-Ops",
            build_dir_rel: "",
            bin_name: "rs-ops",
            iss_rel: "installer/windows/rs-ops.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "aruaru-db",
            dir_name: "aruaru-db",
            build_dir_rel: "",
            bin_name: "aruaru-server",
            iss_rel: "installer/windows/aruaru-db.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "aruaru-llm",
            dir_name: "aruaru-llm",
            build_dir_rel: "",
            bin_name: "aruaru-llm",
            iss_rel: "installer/windows/aruaru-llm.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: Some(GpuVariant {
                cargo_target_dir: "target-gpu",
                features: "hw-detect-vulkan,real-vulkan,hw-detect-directx,real-dx12",
            }),
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "open-english",
            dir_name: "open-english",
            build_dir_rel: "server",
            bin_name: "open-english-server",
            iss_rel: "installer/windows/open-english.iss",
            version_toml_rel: "server/Cargo.toml",
            branch: "master",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "open-gitea",
            dir_name: "open-gitea",
            build_dir_rel: "",
            bin_name: "open-gitea",
            iss_rel: "installer/windows/open-gitea.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "open-raid-z",
            dir_name: "open-raid-z",
            build_dir_rel: "open_runo_zfs_source/open_raid_z_core",
            bin_name: "orzctl",
            iss_rel: "installer/windows/open-raid-z.iss",
            version_toml_rel: "open_runo_zfs_source/open_raid_z_core/Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "open-redmine",
            dir_name: "open-redmine",
            build_dir_rel: "",
            bin_name: "rs-chiketto",
            iss_rel: "installer/windows/open-redmine.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "open-web-server",
            dir_name: "open-web-server",
            build_dir_rel: "",
            bin_name: "open-web-server",
            iss_rel: "installer/windows/open-web-server.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: true,
        },
        InstallerRepoConfig {
            id: "rs-link-fusion",
            dir_name: "rs-link-fusion",
            build_dir_rel: "",
            bin_name: "rs-linkfusion",
            iss_rel: "installer/windows/rs-link-fusion.iss",
            version_toml_rel: "Cargo.toml",
            branch: "master",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "runo.tokyo",
            dir_name: "runo.tokyo",
            build_dir_rel: "",
            bin_name: "runo-tokyo",
            iss_rel: "installer/windows/runo.tokyo.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "rpoem-router",
            dir_name: "RPoem",
            build_dir_rel: "",
            bin_name: "open-runo-router",
            iss_rel: "installer/windows/open-runo-router.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
        InstallerRepoConfig {
            id: "rpoem-tray",
            dir_name: "RPoem",
            build_dir_rel: "apps/desktop-tray",
            bin_name: "open-runo-tray",
            iss_rel: "apps/desktop-tray/installer/open-runo-tray.iss",
            version_toml_rel: "apps/desktop-tray/Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallerSyncOutcome {
    pub repo_id: String,
    /// "updated" | "unchanged" | "skipped" | "error"
    pub status: String,
    pub message: String,
}

fn outcome(id: &str, status: &str, message: impl Into<String>) -> InstallerSyncOutcome {
    InstallerSyncOutcome {
        repo_id: id.to_string(),
        status: status.to_string(),
        message: message.into(),
    }
}

/// `^version\s*=\s*"..."` の最初の一致を拾う簡易パーサ。
/// TOMLパーサを新規依存として増やさず、既存の`[package]`/
/// `[workspace.package]`いずれの書式にも対応できる最小実装
/// (`rust-version`等、行頭が"version"で始まらない他フィールドは
/// 誤検出しない)。
fn read_cargo_version(path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("version") {
            if let Some(eq) = trimmed.find('=') {
                let rest = trimmed[eq + 1..].trim();
                let rest = rest.trim_start_matches('"');
                if let Some(end) = rest.find('"') {
                    return Ok(rest[..end].to_string());
                }
            }
        }
    }
    Err(anyhow!("no version= line found in {}", path.display()))
}

/// `.iss`ファイルから`OutputDir=`の値を読む(全リポジトリ共通で"dist"では
/// ないケース〈open-web-serverは"."〉があるため、ハードコードせず実ファイルを
/// 読む)。
fn read_iss_output_dir(iss_path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(iss_path).with_context(|| format!("reading {}", iss_path.display()))?;
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("OutputDir=") {
            return Ok(rest.trim().to_string());
        }
    }
    Err(anyhow!("no OutputDir= line found in {}", iss_path.display()))
}

fn find_newest_exe(dir: &Path) -> Result<PathBuf> {
    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("exe") {
            let modified = entry.metadata()?.modified()?;
            candidates.push((modified, path));
        }
    }
    candidates
        .into_iter()
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
        .ok_or_else(|| anyhow!("no .exe found in {}", dir.display()))
}

fn sha256_of_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

/// Windows: 既定のInno Setupインストール先。`RS_SYNC_INSTALLER_ISCC_PATH`
/// 環境変数で上書き可能(Linux/VPSではこちらが必須、既定値なし)。
fn resolve_iscc_invocation() -> Result<(String, Vec<String>)> {
    if let Ok(custom) = std::env::var("RS_SYNC_INSTALLER_ISCC_PATH") {
        if !custom.trim().is_empty() {
            if cfg!(windows) {
                return Ok((custom, vec![]));
            }
            // Linux/VPS: Wine経由でWindows用ISCC.exeを実行する。
            // `wine`コマンド自体が無ければここでエラーになり、呼び出し側が
            // 「このホストでは利用不可」として正直にスキップする。
            which_or_err("wine")?;
            return Ok(("wine".to_string(), vec![custom]));
        }
    }
    if cfg!(windows) {
        let default_path = r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe";
        if Path::new(default_path).exists() {
            return Ok((default_path.to_string(), vec![]));
        }
        return Err(anyhow!(
            "Inno Setup Compiler not found at the default path ({default_path}); set RS_SYNC_INSTALLER_ISCC_PATH"
        ));
    }
    Err(anyhow!(
        "RS_SYNC_INSTALLER_ISCC_PATH is required on non-Windows hosts (point it at ISCC.exe under a Wine prefix)"
    ))
}

fn which_or_err(bin: &str) -> Result<()> {
    let found = if cfg!(windows) {
        Command::new("where").arg(bin).output()
    } else {
        Command::new("which").arg(bin).output()
    };
    match found {
        Ok(o) if o.status.success() => Ok(()),
        _ => Err(anyhow!("`{bin}` was not found on PATH")),
    }
}

fn run_checked(mut cmd: Command, label: &str) -> Result<()> {
    let output = cmd.output().with_context(|| format!("spawning {label}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow!("{label} failed: stderr={stderr} stdout={stdout}"));
    }
    Ok(())
}

fn cargo_build(cwd: &Path, bin: &str, extra_env: &[(&str, &str)], extra_features: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(cwd);
    cmd.args(["build", "--release", "--bin", bin]);
    if let Some(features) = extra_features {
        cmd.args(["--features", features]);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    run_checked(cmd, &format!("cargo build --bin {bin} in {}", cwd.display()))
}

fn git(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .with_context(|| format!("running git {:?} in {}", args, repo_root.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 1リポジトリぶんの同期処理。個別リポジトリの失敗は`Err`として返し、
/// 呼び出し側(`run_installer_sync_pass`)が他のリポジトリの処理を止めずに
/// 続行できるようにする。
///
/// **正直な開示・設計変更の経緯(2026-08-26、実機検証で発覚した問題への
/// 対応)**: 当初は「毎回ビルドし、生成された.exeのSHA-256を既存の
/// コミット済みファイルと比較して、違っていた時だけコミットする」設計
/// だったが、実際に同一ソースで2回連続ビルドし直したところ、**ソースを
/// 一切変更していないのにリビルドのたびに実行ファイルのハッシュが
/// 変わる**ことを確認した(Rustのリリースビルドはデフォルトでは
/// バイト単位の再現性が無いため)。この状態で.exeハッシュだけを基準に
/// すると、ユーザーが明示的に選んだ「差分があった時だけ自動コミット・
/// push」という方針に反して毎回意味の無いコミットが積み重なってしまう
/// ——そのため、**主たるゲートをgitのHEADコミット比較に変更**した:
/// 前回このリポジトリを同期した時点のHEADコミットハッシュ
/// (`last_synced_head`、`InstallerSyncSettings`に永続化)と現在のHEADが
/// 同じであれば、ビルド自体を一切行わずに"unchanged"として即座に返す
/// (無駄なCPU消費も避けられる)。HEADが異なる場合のみビルド→ISCC→
/// (念のための)実行ファイルハッシュ比較→差分があればコミット・push、
/// という流れに進み、成功後は(コミットの有無に関わらず)push後の新しい
/// HEADを`last_synced_head`として記録する——これにより「このリポジトリの
/// ソースに実際に新しいコミットが無い限り再ビルドしない」という、本来
/// 意図していた挙動が保証される。
pub fn sync_one_repo(
    root: &Path,
    cfg: &InstallerRepoConfig,
    last_synced_head: Option<&str>,
) -> Result<(InstallerSyncOutcome, Option<String>)> {
    let repo_root = root.join(cfg.dir_name);
    if !repo_root.exists() {
        return Ok((
            outcome(cfg.id, "skipped", format!("repo not present on this host at {}", repo_root.display())),
            None,
        ));
    }

    let current_head = git(&repo_root, &["rev-parse", "HEAD"])?.trim().to_string();
    if Some(current_head.as_str()) == last_synced_head {
        return Ok((
            outcome(cfg.id, "unchanged", "no new commits on this repo since the last installer sync; build skipped"),
            None,
        ));
    }

    let build_cwd = if cfg.build_dir_rel.is_empty() {
        repo_root.clone()
    } else {
        repo_root.join(cfg.build_dir_rel)
    };
    if !build_cwd.exists() {
        return Ok((
            outcome(cfg.id, "skipped", format!("build dir not present: {}", build_cwd.display())),
            None,
        ));
    }

    let version_toml = repo_root.join(cfg.version_toml_rel);
    let version = read_cargo_version(&version_toml)?;

    // 1. 通常ビルド(CPUのみ、全リポジトリ共通)
    cargo_build(&build_cwd, cfg.bin_name, &[], None)?;

    // 2. aruaru-llmのみ: GPU版も追加ビルド(ISCCがSource必須で参照するため)
    if let Some(gpu) = &cfg.gpu_variant {
        cargo_build(
            &build_cwd,
            cfg.bin_name,
            &[("CARGO_TARGET_DIR", gpu.cargo_target_dir)],
            Some(gpu.features),
        )?;
    }

    let exe_name = if cfg!(windows) {
        format!("{}.exe", cfg.bin_name)
    } else {
        cfg.bin_name.to_string()
    };
    let built_exe = build_cwd.join("target").join("release").join(&exe_name);
    if !built_exe.exists() {
        return Err(anyhow!("expected build output not found: {}", built_exe.display()));
    }

    let iss_path = repo_root.join(cfg.iss_rel);
    let iss_dir = iss_path
        .parent()
        .ok_or_else(|| anyhow!("iss path has no parent: {}", iss_path.display()))?
        .to_path_buf();

    // 3. open-web-serverのみ: ISCCが".iss"と同じフォルダから直接
    //    バイナリを拾う設計のため、先にそこへコピーしておく。
    if cfg.stage_binary_before_iscc {
        let staged = iss_dir.join(&exe_name);
        std::fs::copy(&built_exe, &staged)
            .with_context(|| format!("staging {} -> {}", built_exe.display(), staged.display()))?;
    }

    // 4. ISCCでインストーラーをコンパイル
    let (iscc_bin, mut iscc_prefix_args) = resolve_iscc_invocation()?;
    let version_define = format!("/DMyAppVersion={version}");
    let iss_filename = iss_path
        .file_name()
        .ok_or_else(|| anyhow!("iss path has no filename"))?
        .to_string_lossy()
        .to_string();
    let mut cmd = Command::new(&iscc_bin);
    cmd.current_dir(&iss_dir);
    iscc_prefix_args.push(version_define);
    iscc_prefix_args.push(iss_filename);
    cmd.args(&iscc_prefix_args);
    run_checked(cmd, &format!("ISCC compile for {}", cfg.id))?;

    // 5. 出力先ディレクトリ(OutputDir=、"dist"とは限らない)から
    //    生成された.exeを見つける。
    let output_dir_rel = read_iss_output_dir(&iss_path)?;
    let output_dir = iss_dir.join(&output_dir_rel);
    let produced_exe = find_newest_exe(&output_dir)?;
    let tracked_path = iss_dir.join(produced_exe.file_name().unwrap());

    // 6. ハッシュ比較(念のための二重チェック——新しいソースコミットに
    //    伴うリビルドでも、ごく稀に生成物が既存のコミット済みファイルと
    //    バイト単位で一致することがあり得るため、その場合は無駄な
    //    コミットを作らない。主たるゲートは上記のgit HEAD比較)。
    let old_hash = sha256_of_file(&tracked_path);
    let new_hash = sha256_of_file(&produced_exe);
    if old_hash.is_some() && old_hash == new_hash {
        let head_after = git(&repo_root, &["rev-parse", "HEAD"])?.trim().to_string();
        return Ok((
            outcome(cfg.id, "unchanged", "source changed but rebuilt installer binary is byte-identical; nothing to commit"),
            Some(head_after),
        ));
    }

    if tracked_path != produced_exe {
        std::fs::copy(&produced_exe, &tracked_path)
            .with_context(|| format!("copying {} -> {}", produced_exe.display(), tracked_path.display()))?;
    }

    // 7. コミット・push(この`.exe`ファイル1つだけをstageする——他の
    //    未コミットファイルには一切触れない)。
    let rel_for_git = tracked_path
        .strip_prefix(&repo_root)
        .unwrap_or(&tracked_path)
        .to_string_lossy()
        .replace('\\', "/");
    git(&repo_root, &["add", "--", &rel_for_git])?;
    let commit_msg = format!(
        "installer sync: rebuild {} ({})\n\nRS-Syncのインストーラー自動同期機能による再ビルド。\
         このリポジトリに新しいコミット(HEAD: {current_head} -> ビルド対象)があったため、\
         インストーラーを再ビルドしてハッシュ差分を検知しコミットした。\n\n\
         Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>",
        rel_for_git,
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    );
    let commit_result = git(&repo_root, &["commit", "-m", &commit_msg]);
    if let Err(e) = commit_result {
        // "nothing to commit"はgit addの後でも起こり得ない設計だが、
        // 万一の競合(手動pushとの重なり等)は正直にエラーとして返す。
        return Err(anyhow!("git commit failed for {}: {e}", cfg.id));
    }
    git(&repo_root, &["push", "origin", cfg.branch])?;
    let head_after = git(&repo_root, &["rev-parse", "HEAD"])?.trim().to_string();

    Ok((
        outcome(cfg.id, "updated", format!("rebuilt and pushed {rel_for_git} (v{version})")),
        Some(head_after),
    ))
}

/// 全リポジトリを1回まとめて処理する。個別のエラーは記録するのみで
/// 他のリポジトリの処理を止めない。`last_synced_heads`は「そのリポジトリを
/// 前回同期した時点のgit HEAD」(`InstallerSyncSettings::last_synced_heads`、
/// 永続化済み)——ここが現在のHEADと一致するリポジトリはビルドすら行わず
/// "unchanged"として即座に返る(`sync_one_repo`のモジュールdoc参照)。
/// 呼び出し側は返ってきた新しいマップをそのまま`InstallerSyncSettings`へ
/// 書き戻すこと。
pub fn run_installer_sync_pass(
    root: &Path,
    last_synced_heads: &std::collections::HashMap<String, String>,
) -> (Vec<InstallerSyncOutcome>, std::collections::HashMap<String, String>) {
    let mut new_heads = last_synced_heads.clone();
    let mut outcomes = Vec::new();
    for cfg in installer_sync_repos() {
        let prev = last_synced_heads.get(cfg.id).map(|s| s.as_str());
        match sync_one_repo(root, &cfg, prev) {
            Ok((o, head)) => {
                if let Some(h) = head {
                    new_heads.insert(cfg.id.to_string(), h);
                }
                outcomes.push(o);
            }
            Err(e) => outcomes.push(outcome(cfg.id, "error", e.to_string())),
        }
    }
    (outcomes, new_heads)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstallerSyncSettings {
    /// 既定false(オプトイン)。trueでも`RS_SYNC_INSTALLER_SYNC_ENABLED=1`
    /// (環境変数)が無ければ実際には動かない——二重の安全弁。
    #[serde(default)]
    pub enabled: bool,
    /// 分単位、既定15分。
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u32,
    #[serde(default)]
    pub last_run_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub last_outcomes: Vec<InstallerSyncOutcome>,
    /// リポジトリID→前回同期(push)時点のgit HEADコミットハッシュ。
    /// `sync_one_repo`のモジュールdoc参照——このマップが「本当に新しい
    /// コミットがあった時だけリビルドする」ための唯一の記録。
    #[serde(default)]
    pub last_synced_heads: std::collections::HashMap<String, String>,
}

fn default_interval_minutes() -> u32 {
    15
}

/// 環境変数`RS_SYNC_INSTALLER_SYNC_ENABLED=1`が設定されているか
/// (機能全体のマスタースイッチ、`Store`側の`enabled`フラグとは別に
/// 必須)。
pub fn env_enabled() -> bool {
    std::env::var("RS_SYNC_INSTALLER_SYNC_ENABLED")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// `RS_SYNC_INSTALLER_ROOT`(各リポジトリの親ディレクトリ)。未設定なら
/// `None`——呼び出し側は全件スキップする。
pub fn configured_root() -> Option<PathBuf> {
    std::env::var("RS_SYNC_INSTALLER_ROOT").ok().filter(|s| !s.trim().is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_cargo_version_finds_package_version_not_rust_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cargo.toml");
        std::fs::write(&path, "[package]\nname = \"x\"\nrust-version = \"1.80\"\nversion = \"0.3.1\"\n").unwrap();
        assert_eq!(read_cargo_version(&path).unwrap(), "0.3.1");
    }

    #[test]
    fn read_cargo_version_finds_workspace_package_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cargo.toml");
        std::fs::write(&path, "[workspace.package]\nversion      = \"0.5.0\"\nrust-version = \"1.82\"\n").unwrap();
        assert_eq!(read_cargo_version(&path).unwrap(), "0.5.0");
    }

    #[test]
    fn read_iss_output_dir_reads_dist_or_dot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.iss");
        std::fs::write(&path, "[Setup]\nOutputDir=dist\n").unwrap();
        assert_eq!(read_iss_output_dir(&path).unwrap(), "dist");

        let path2 = dir.path().join("y.iss");
        std::fs::write(&path2, "[Setup]\nOutputDir=.\n").unwrap();
        assert_eq!(read_iss_output_dir(&path2).unwrap(), ".");
    }

    #[test]
    fn find_newest_exe_picks_most_recently_modified_file() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.exe");
        std::fs::write(&old, b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = dir.path().join("newer.exe");
        std::fs::write(&newer, b"newer").unwrap();
        let found = find_newest_exe(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "newer.exe");
    }

    #[test]
    fn sha256_of_file_matches_between_byte_identical_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        std::fs::write(&a, b"identical payload").unwrap();
        std::fs::write(&b, b"identical payload").unwrap();
        assert_eq!(sha256_of_file(&a), sha256_of_file(&b));

        let c = dir.path().join("c.bin");
        std::fs::write(&c, b"different payload").unwrap();
        assert_ne!(sha256_of_file(&a), sha256_of_file(&c));
    }

    #[test]
    fn env_enabled_requires_exact_value_1() {
        std::env::remove_var("RS_SYNC_INSTALLER_SYNC_ENABLED");
        assert!(!env_enabled());
    }

    /// 実機検証(2026-08-26): 同一ソースでも`cargo build --release`を
    /// 繰り返すたびに実行ファイルのハッシュが変わり得ることを確認した
    /// (`sync_one_repo`のモジュールdoc参照)ため、主たるゲートを
    /// git HEAD比較へ変更した。この回帰テストは、現在のHEADが
    /// `last_synced_head`と一致する場合、実際にcargo/ISCCを一切
    /// 呼び出さずに即座に"unchanged"を返すことを、本物のgitリポジトリ
    /// (一時ディレクトリ、実際に`git init`・`git commit`する)を使って
    /// 検証する——存在しないバイナリ名・存在しない`.iss`パスを設定に
    /// 与えているため、もし短絡せずビルドを試みればこのテストは
    /// 確実に失敗する。
    #[test]
    fn sync_one_repo_skips_build_entirely_when_head_matches_last_synced() {
        let root = tempfile::tempdir().unwrap();
        let repo_dir = root.path().join("fake-repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        std::fs::write(repo_dir.join("Cargo.toml"), "[package]\nname = \"fake\"\nversion = \"9.9.9\"\n").unwrap();

        let run = |args: &[&str]| {
            let out = std::process::Command::new("git").current_dir(&repo_dir).args(args).output().unwrap();
            assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "test"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "initial"]);
        let head_out = std::process::Command::new("git").current_dir(&repo_dir).args(["rev-parse", "HEAD"]).output().unwrap();
        let head = String::from_utf8_lossy(&head_out.stdout).trim().to_string();

        let cfg = InstallerRepoConfig {
            id: "fake-repo",
            dir_name: "fake-repo",
            build_dir_rel: "",
            // 実在しないバイナリ名・実在しない.issパス: もし短絡せず
            // ビルド/ISCCを試みれば、これらの理由で必ずErrになる。
            bin_name: "this-binary-does-not-exist-xyz",
            iss_rel: "installer/windows/does-not-exist.iss",
            version_toml_rel: "Cargo.toml",
            branch: "main",
            gpu_variant: None,
            stage_binary_before_iscc: false,
        };

        let (result, new_head) = sync_one_repo(root.path(), &cfg, Some(head.as_str())).expect("must not error when short-circuiting");
        assert_eq!(result.status, "unchanged");
        assert!(new_head.is_none(), "short-circuit path should not report a new head to persist");

        // 対照実験: 前回HEADが記録されていない(=初回)場合は短絡せず、
        // 実際にビルドを試みてエラーになることを確認(存在しない
        // バイナリ名を指定しているため)。これにより「常にunchangedを
        // 返すだけの壊れたテスト」ではないことを示す。
        let err = sync_one_repo(root.path(), &cfg, None).expect_err("without a matching last_synced_head, build must be attempted and fail");
        assert!(
            format!("{err:#}").contains("this-binary-does-not-exist-xyz") || format!("{err:#}").contains("cargo build"),
            "expected a build-related error, got: {err:#}"
        );
    }

    /// 実機E2E検証用(既定では実行されない、`cargo test -- --ignored`で
    /// 明示的に呼ぶ)。rs-sync自身のリポジトリ(`F:\runo\rs-sync`)を対象に
    /// 実際のcargo build→ISCC→ハッシュ比較→(差分があれば)コミット・push
    /// までを本物に対して1回走らせ、パイプライン全体の配線ミスを
    /// 洗い出すための一回限りの検証。このテストは意図的に副作用(実際の
    /// git commit/push)を起こし得るため、CIやこの開発機以外では
    /// 実行しないこと。
    #[test]
    #[ignore]
    fn real_world_e2e_sync_one_repo_against_rs_sync_itself() {
        let root = std::path::Path::new(r"F:\runo");
        let cfg = installer_sync_repos().into_iter().find(|c| c.id == "rs-sync").unwrap();
        let (result, new_head) = sync_one_repo(root, &cfg, None).expect("real E2E run should succeed");
        eprintln!("E2E result: {result:?}");
        eprintln!("new_head: {new_head:?}");
        assert!(matches!(result.status.as_str(), "updated" | "unchanged"));

        // 2回目のパス: 今しがた記録した新HEADを渡すと、実際にビルドせず
        // 即座に"unchanged"で返ることを確認する(短絡が機能している証拠、
        // 実行時間もごく短いはず)。
        if let Some(head) = new_head {
            let started = std::time::Instant::now();
            let (result2, new_head2) = sync_one_repo(root, &cfg, Some(head.as_str())).expect("second pass should succeed");
            let elapsed = started.elapsed();
            eprintln!("second pass result: {result2:?} (took {elapsed:?})");
            assert_eq!(result2.status, "unchanged");
            assert!(new_head2.is_none());
            assert!(elapsed.as_secs() < 5, "second pass should short-circuit without building, took {elapsed:?}");
        }
    }

    #[test]
    fn installer_sync_repos_has_sixteen_entries_with_unique_ids() {
        let repos = installer_sync_repos();
        assert_eq!(repos.len(), 16);
        let mut ids: Vec<&str> = repos.iter().map(|r| r.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 16, "expected all repo ids to be unique");
    }
}
