use crate::model::{Account, Direction, SyncJob};
use crate::provider::provider_for;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Per-repo state used for two-way conflict detection: the commit sha each
/// side was at as of the last successful (non-conflicted) sync.
/// Simplification, documented honestly: this compares only the HEAD sha of
/// each remote's default branch, not every branch/tag. Good enough for the
/// MVP's "did anything change on either side since last sync" check.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TwoWayState {
    pub last_synced: HashMap<String, String>, // repo -> sha
}

fn state_path(job_id: &str) -> PathBuf {
    PathBuf::from(crate::model::DATA_DIR)
        .join("state")
        .join(format!("{job_id}.json"))
}

fn load_state(job_id: &str) -> TwoWayState {
    let p = state_path(job_id);
    if let Ok(raw) = fs::read_to_string(&p) {
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        TwoWayState::default()
    }
}

fn save_state(job_id: &str, state: &TwoWayState) {
    let p = state_path(job_id);
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = fs::write(p, json);
    }
}

fn cache_dir(job_id: &str, repo: &str) -> PathBuf {
    PathBuf::from(crate::model::DATA_DIR)
        .join("cache")
        .join(job_id)
        .join(format!("{repo}.git"))
}

/// **重大な実セキュリティ障害の修正(2026-07-30、ユーザー実機報告で発覚)**:
/// リモートURLに`https://<token>@github.com/...`の形でトークンを埋め込む
/// 既存方式(`provider.rs`)自体は変更していないが、`run_git`が失敗時に
/// `args`(トークン入りURLを含むコマンド引数そのもの)と`git`の標準
/// エラー出力(こちらもリモートURLをそのままエコーする)を**無加工で
/// エラーメッセージに含めていた**ため、同期ジョブが失敗するたびに、
/// 実際のGitHub PAT等が本番UI(ジョブの実行結果メッセージ)へ平文で
/// 表示されてしまっていた——ユーザーが実際に本番環境で踏んだ実障害。
/// `redact_credentials`でリモートURL中の`scheme://user:pass@`/
/// `scheme://token@`部分を`***`に置き換えてから返す。
fn redact_credentials(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(scheme_pos) = rest.find("://") {
        let (before_scheme, after_scheme_marker) = rest.split_at(scheme_pos);
        let after_scheme = &after_scheme_marker[3..]; // skip "://"
        out.push_str(before_scheme);
        out.push_str("://");
        // Credentials, if any, run from here up to the next '@' -- but only
        // if that '@' appears before the next '/' (otherwise "@" belongs to
        // the host/path, not a credential separator, and there is nothing to
        // redact).
        let boundary = after_scheme.find('/').unwrap_or(after_scheme.len());
        match after_scheme[..boundary].find('@') {
            Some(at_pos) => {
                out.push_str("***@");
                rest = &after_scheme[at_pos + 1..];
            }
            None => {
                rest = after_scheme;
            }
        }
    }
    out.push_str(rest);
    out
}

fn run_git(args: &[&str], cwd: Option<&std::path::Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        let redacted_args: Vec<String> = args.iter().map(|a| redact_credentials(a)).collect();
        let redacted_stderr = redact_credentials(&String::from_utf8_lossy(&output.stderr));
        return Err(anyhow!("git {:?} failed: {}", redacted_args, redacted_stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// clone --mirror if the cache doesn't exist yet, otherwise fetch into it.
fn mirror_fetch(cache: &PathBuf, remote_url: &str) -> Result<()> {
    if cache.exists() {
        run_git(&["remote", "set-url", "origin", remote_url], Some(cache))?;
        run_git(&["fetch", "--prune", "origin", "+refs/*:refs/*"], Some(cache))?;
    } else {
        if let Some(parent) = cache.parent() {
            fs::create_dir_all(parent)?;
        }
        run_git(
            &["clone", "--mirror", remote_url, cache.to_str().unwrap()],
            None,
        )?;
    }
    Ok(())
}

/// **実バグ修正(2026-07-30、ユーザーの本番実行結果で判明)**: 従来
/// `git push --mirror`をそのまま使っており、`mirror_fetch`が
/// `+refs/*:refs/*`で取得した**GitHub自身が管理する`refs/pull/*`
/// (プルリクエストの参照)まで送信先へpushしようとして、GitHub側の
/// 「hidden refへの書き込み拒否」で毎回エラーになっていた
/// (`refs/pull/*`はどのリポジトリでもGitHubが自動生成・管理する
/// ものであり、そもそも外部から書き込めるものではない——バグという
/// より、`--mirror`が意図せず対象外のrefまで含めてしまっていた設計上の
/// 見落とし)。実際に同期すべきなのはブランチ(`refs/heads/*`)とタグ
/// (`refs/tags/*`)のみのため、明示的にこの2つだけを対象にする
/// (`--prune`で送信先にのみ存在する古いブランチ/タグの削除は維持、
/// `--mirror`の「宛先を完全に同一化する」という目的自体は保つ)。
fn mirror_push(cache: &PathBuf, remote_url: &str) -> Result<()> {
    run_git(
        &["push", "--prune", remote_url, "+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"],
        Some(cache),
    )?;
    Ok(())
}

/// The bare mirror's `HEAD` symref is only reliable if it happens to point at
/// a branch that actually has commits. In practice a freshly `git init
/// --bare`'d repo's `HEAD` defaults to whatever `init.defaultBranch` is
/// configured on the machine that created it (historically `master`), which
/// may not match the branch this codebase actually pushes to (`main`). When
/// that happens, plain `git rev-parse HEAD` doesn't fail (exit code 0) --
/// it just echoes the literal, unresolved string `"HEAD"` back, which then
/// silently poisons every last-synced-sha comparison downstream (a real bug,
/// found via `tests/sync_integration.rs`'s two-way conflict-detection test).
/// Resolve explicitly against `refs/heads/main` first, falling back to
/// `refs/heads/master`, mirroring the same fallback already used for the
/// dest-side comparison a few lines below.
fn head_sha(cache: &PathBuf) -> Result<String> {
    run_git(&["rev-parse", "refs/heads/main"], Some(cache))
        .or_else(|_| run_git(&["rev-parse", "refs/heads/master"], Some(cache)))
        .map(|s| s.trim().to_string())
}

pub fn resolve_repos(job: &SyncJob, source: &Account) -> Result<Vec<String>> {
    if !job.repos.is_empty() {
        return Ok(job.repos.clone());
    }
    let provider = provider_for(&source.provider);
    provider.list_repos(source)
}

pub struct RunOutcome {
    pub message: String,
    pub success: bool,
}

/// Execute one job run: for each target repo, mirror-sync source -> dest
/// (one-way), or detect+apply changes both directions with a simple
/// last-synced-sha conflict check (two-way).
pub fn run_job(job: &mut SyncJob, source: &Account, dest: &Account) -> RunOutcome {
    let repos = match resolve_repos(job, source) {
        Ok(r) => r,
        Err(e) => {
            return RunOutcome {
                success: false,
                message: format!("repo discovery failed: {e}"),
            }
        }
    };

    let source_provider = provider_for(&source.provider);
    let dest_provider = provider_for(&dest.provider);
    let mut messages = Vec::new();
    let mut any_failure = false;
    let mut state = load_state(&job.id);

    for repo in &repos {
        if job.conflicted_repos.contains(repo) {
            messages.push(format!(
                "{repo}: skipped (conflicted, needs manual resolution)"
            ));
            continue;
        }

        let result = (|| -> Result<String> {
            let src_url = source_provider.remote_url(source, repo);
            let dst_url = dest_provider.remote_url(dest, repo);
            dest_provider.ensure_repo(dest, repo)?;

            let cache = cache_dir(&job.id, repo);
            mirror_fetch(&cache, &src_url)?;
            let src_sha = head_sha(&cache).unwrap_or_default();

            match job.direction {
                Direction::OneWay => {
                    mirror_push(&cache, &dst_url)?;
                    state.last_synced.insert(repo.clone(), src_sha.clone());
                    Ok(format!("{repo}: one-way source->dest ok ({src_sha})"))
                }
                Direction::TwoWay => {
                    // Fetch dest HEAD via a throwaway remote ref to compare.
                    run_git(
                        &["fetch", "--prune", &dst_url, "+refs/*:refs/dest/*"],
                        Some(&cache),
                    )?;
                    let dst_sha =
                        run_git(&["rev-parse", "refs/dest/heads/main"], Some(&cache))
                            .or_else(|_| run_git(&["rev-parse", "refs/dest/heads/master"], Some(&cache)))
                            .unwrap_or_default()
                            .trim()
                            .to_string();

                    let last = state.last_synced.get(repo).cloned().unwrap_or_default();
                    let src_changed = src_sha != last && !src_sha.is_empty();
                    let dst_changed = dst_sha != last && !dst_sha.is_empty();

                    if src_changed && dst_changed && src_sha != dst_sha {
                        job.conflicted_repos.push(repo.clone());
                        return Ok(format!(
                            "{repo}: CONFLICT detected (both sides changed since last sync); \
                             auto-run paused for this repo, resolve manually"
                        ));
                    }
                    if src_changed {
                        mirror_push(&cache, &dst_url)?;
                        state.last_synced.insert(repo.clone(), src_sha.clone());
                        Ok(format!("{repo}: two-way source->dest ok ({src_sha})"))
                    } else if dst_changed {
                        // `cache` currently mirrors *source*'s refs (from the
                        // `mirror_fetch` above) -- the dest-only commits we
                        // detected only exist under the throwaway
                        // `refs/dest/*` namespace from the comparison fetch,
                        // not on the cache's normal refs. Pushing `cache` as-is
                        // to `src_url` would therefore be a silent no-op (it
                        // would just push source's own unchanged state back to
                        // itself). Re-fetch a real mirror of dest's refs into
                        // the cache first so the subsequent push actually
                        // carries dest's changes.
                        mirror_fetch(&cache, &dst_url)?;
                        mirror_push(&cache, &src_url)?;
                        state.last_synced.insert(repo.clone(), dst_sha.clone());
                        Ok(format!("{repo}: two-way dest->source ok ({dst_sha})"))
                    } else {
                        Ok(format!("{repo}: no changes"))
                    }
                }
            }
        })();

        match result {
            Ok(msg) => messages.push(msg),
            Err(e) => {
                any_failure = true;
                messages.push(format!("{repo}: FAILED: {e}"));
            }
        }
    }

    save_state(&job.id, &state);

    RunOutcome {
        success: !any_failure,
        message: messages.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_credentials_removes_embedded_token_from_https_url() {
        let input = "git [\"push\", \"--mirror\", \"https://ghp_REDACTED0000000000000000000000000000@github.com/OpenRuno/aruaru-web.git\"] failed";
        let redacted = redact_credentials(input);
        assert!(!redacted.contains("ghp_REDACTED0000000000000000000000000000"), "token must not survive redaction: {redacted}");
        assert!(redacted.contains("https://***@github.com/OpenRuno/aruaru-web.git"), "redacted form should keep the rest of the URL intact: {redacted}");
    }

    #[test]
    fn redact_credentials_handles_multiple_urls_in_the_same_string() {
        let input = "from https://tok1@github.com/a/b.git to https://tok2@github.com/c/d.git";
        let redacted = redact_credentials(input);
        assert!(!redacted.contains("tok1") && !redacted.contains("tok2"));
        assert_eq!(redacted, "from https://***@github.com/a/b.git to https://***@github.com/c/d.git");
    }

    #[test]
    fn redact_credentials_leaves_urls_without_credentials_unchanged() {
        let input = "cloning https://github.com/aon-co-jp/rs-sync.git into /tmp/x";
        assert_eq!(redact_credentials(input), input);
    }

    #[test]
    fn redact_credentials_does_not_touch_at_signs_that_are_not_credential_separators() {
        // An '@' appearing in the path (after the first '/') is not a
        // credential separator and must be left alone.
        let input = "see https://example.test/users/@handle for details";
        assert_eq!(redact_credentials(input), input);
    }

    #[test]
    fn redact_credentials_leaves_plain_text_without_urls_unchanged() {
        let input = "no urls here at all";
        assert_eq!(redact_credentials(input), input);
    }
}
