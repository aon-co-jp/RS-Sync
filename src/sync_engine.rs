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

fn run_git(args: &[&str], cwd: Option<&std::path::Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        ));
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

fn mirror_push(cache: &PathBuf, remote_url: &str) -> Result<()> {
    run_git(&["push", "--mirror", remote_url], Some(cache))?;
    Ok(())
}

fn head_sha(cache: &PathBuf) -> Result<String> {
    let out = run_git(&["rev-parse", "HEAD"], Some(cache))?;
    Ok(out.trim().to_string())
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
