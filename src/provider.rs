use crate::model::{Account, ProviderKind};
use anyhow::{anyhow, Result};
use serde::Deserialize;

/// Abstraction over a git-hosting provider's REST API for repo discovery/creation.
/// Actual clone/push traffic always goes through the standard git smart-HTTP
/// protocol via the `git` CLI (see sync_engine.rs), so providers only need to
/// answer "what repos exist" / "create this repo" here.
pub trait Provider {
    /// List repo names (without owner prefix) under the account's owner/org.
    fn list_repos(&self, account: &Account) -> Result<Vec<String>>;
    /// Create a repo if it doesn't already exist. Returns Ok(()) either way
    /// unless the create call itself fails.
    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()>;
    /// Authenticated git clone/push URL for a given repo.
    fn remote_url(&self, account: &Account, repo: &str) -> String;
}

pub fn provider_for(kind: &ProviderKind) -> Box<dyn Provider> {
    match kind {
        ProviderKind::GitHub => Box::new(GitHubProvider),
        ProviderKind::OpenGitea => Box::new(OpenGiteaProvider),
        ProviderKind::Gitea => Box::new(GiteaProvider),
        ProviderKind::Gitbucket => Box::new(GitbucketProvider),
        ProviderKind::Local => Box::new(LocalProvider),
    }
}

// ---------------- Local (test/dev only, not a real hosting provider) ----------------

pub struct LocalProvider;

impl Provider for LocalProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&account.git_base) {
            for e in entries.flatten() {
                if let Some(name) = e.file_name().to_str() {
                    if let Some(stripped) = name.strip_suffix(".git") {
                        out.push(stripped.to_string());
                    }
                }
            }
        }
        Ok(out)
    }

    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()> {
        let path = std::path::Path::new(&account.git_base).join(format!("{repo}.git"));
        if !path.exists() {
            std::fs::create_dir_all(&path)?;
            let status = std::process::Command::new("git")
                .args(["init", "--bare", path.to_str().unwrap()])
                .status()?;
            if !status.success() {
                return Err(anyhow!("local ensure_repo: git init --bare failed"));
            }
        }
        Ok(())
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        format!("{}/{}.git", account.git_base.trim_end_matches('/'), repo)
    }
}

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("rs-sync/0.1")
        .build()
        .expect("http client")
}

// ---------------- GitHub (fully implemented) ----------------

pub struct GitHubProvider;

#[derive(Deserialize)]
struct GhRepo {
    name: String,
}

impl Provider for GitHubProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let client = http_client();
        let mut repos = Vec::new();
        let mut page = 1;
        loop {
            let url = format!(
                "{}/orgs/{}/repos?per_page=100&page={}",
                account.api_base.trim_end_matches('/'),
                account.owner,
                page
            );
            let resp = client
                .get(&url)
                .header("Authorization", format!("token {}", account.token))
                .header("Accept", "application/vnd.github+json")
                .send()?;
            // Fall back to the /users/:user/repos endpoint if this isn't an org.
            let resp = if resp.status() == reqwest::StatusCode::NOT_FOUND {
                let user_url = format!(
                    "{}/users/{}/repos?per_page=100&page={}",
                    account.api_base.trim_end_matches('/'),
                    account.owner,
                    page
                );
                client
                    .get(&user_url)
                    .header("Authorization", format!("token {}", account.token))
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            } else {
                resp
            };
            if !resp.status().is_success() {
                return Err(anyhow!("GitHub list_repos failed: {}", resp.status()));
            }
            let batch: Vec<GhRepo> = resp.json()?;
            if batch.is_empty() {
                break;
            }
            let got = batch.len();
            repos.extend(batch.into_iter().map(|r| r.name));
            if got < 100 {
                break;
            }
            page += 1;
        }
        Ok(repos)
    }

    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()> {
        let client = http_client();
        let check_url = format!(
            "{}/repos/{}/{}",
            account.api_base.trim_end_matches('/'),
            account.owner,
            repo
        );
        let resp = client
            .get(&check_url)
            .header("Authorization", format!("token {}", account.token))
            .header("Accept", "application/vnd.github+json")
            .send()?;
        if resp.status().is_success() {
            return Ok(());
        }
        let create_url = format!(
            "{}/orgs/{}/repos",
            account.api_base.trim_end_matches('/'),
            account.owner
        );
        let body = serde_json::json!({ "name": repo, "private": true });
        let resp = client
            .post(&create_url)
            .header("Authorization", format!("token {}", account.token))
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()?;
        if resp.status().is_success() {
            return Ok(());
        }
        // Retry against /user/repos in case owner is a personal account, not an org.
        let user_create_url = format!("{}/user/repos", account.api_base.trim_end_matches('/'));
        let resp = client
            .post(&user_create_url)
            .header("Authorization", format!("token {}", account.token))
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("GitHub ensure_repo failed: {}", resp.status()))
        }
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        // https://<token>@github.com/<owner>/<repo>.git
        let base = account
            .git_base
            .trim_end_matches('/')
            .replacen("https://", "", 1)
            .replacen("http://", "", 1);
        format!("https://{}@{}/{}/{}.git", account.token, base, account.owner, repo)
    }
}

// ---------------- open-gitea (self-hosted, partial) ----------------
// open-gitea (aon-co-jp/open-gitea) exposes git smart-HTTP directly plus a small JSON API
// (`GET /api/repos`, `PUT /repos/:name` to create). No auth token scheme is
// finalized yet on the open-gitea side beyond a fixed REMOTE_USER, so ensure_repo
// here is best-effort and list_repos uses the documented /api/repos endpoint.
pub struct OpenGiteaProvider;

#[derive(Deserialize)]
struct OpenGiteaRepoEntry {
    name: String,
}

impl Provider for OpenGiteaProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let client = http_client();
        let url = format!("{}/api/repos", account.api_base.trim_end_matches('/'));
        let resp = client.get(&url).send()?;
        if !resp.status().is_success() {
            return Err(anyhow!("open-gitea list_repos failed: {}", resp.status()));
        }
        let entries: Vec<OpenGiteaRepoEntry> = resp.json().unwrap_or_default();
        Ok(entries.into_iter().map(|e| e.name).collect())
    }

    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()> {
        let client = http_client();
        let url = format!(
            "{}/repos/{}",
            account.api_base.trim_end_matches('/'),
            repo
        );
        let resp = client.put(&url).send()?;
        // open-gitea returns success or "already exists" style responses; treat any
        // non-5xx as acceptable since this is best-effort per the docs above.
        if resp.status().is_server_error() {
            return Err(anyhow!("open-gitea ensure_repo failed: {}", resp.status()));
        }
        Ok(())
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        format!("{}/{}.git", account.git_base.trim_end_matches('/'), repo)
    }
}

// ---------------- Gitea (documented, not verified live) ----------------
// Gitea's REST API (https://try.gitea.io/api/swagger) uses
// `GET /api/v1/orgs/:org/repos` and `POST /api/v1/orgs/:org/repos` with a
// `token` header. Implemented against the documented shape; not yet
// exercised against a live Gitea instance in this codebase's tests.
pub struct GiteaProvider;

#[derive(Deserialize)]
struct GiteaRepo {
    name: String,
}

impl Provider for GiteaProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let client = http_client();
        let url = format!(
            "{}/api/v1/orgs/{}/repos",
            account.api_base.trim_end_matches('/'),
            account.owner
        );
        let resp = client
            .get(&url)
            .header("Authorization", format!("token {}", account.token))
            .send()?;
        if !resp.status().is_success() {
            return Err(anyhow!("Gitea list_repos failed: {}", resp.status()));
        }
        let repos: Vec<GiteaRepo> = resp.json()?;
        Ok(repos.into_iter().map(|r| r.name).collect())
    }

    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()> {
        let client = http_client();
        let url = format!(
            "{}/api/v1/orgs/{}/repos",
            account.api_base.trim_end_matches('/'),
            account.owner
        );
        let body = serde_json::json!({ "name": repo, "private": true });
        let resp = client
            .post(&url)
            .header("Authorization", format!("token {}", account.token))
            .json(&body)
            .send()?;
        if resp.status().is_success() || resp.status() == reqwest::StatusCode::CONFLICT {
            Ok(())
        } else {
            Err(anyhow!("Gitea ensure_repo failed: {}", resp.status()))
        }
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        let base = account
            .git_base
            .trim_end_matches('/')
            .replacen("https://", "", 1)
            .replacen("http://", "", 1);
        format!(
            "https://{}:{}@{}/{}/{}.git",
            account.owner, account.token, base, account.owner, repo
        )
    }
}

// ---------------- Gitbucket (documented, not verified live) ----------------
// GitBucket exposes a GitHub-API-compatible subset
// (https://github.com/gitbucket/gitbucket/wiki/API-WebHook), so this adapter
// mirrors the GitHub one against `/api/v3/...` paths. Not yet exercised
// against a live GitBucket instance; documented here honestly as unverified.
pub struct GitbucketProvider;

impl Provider for GitbucketProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let client = http_client();
        let url = format!(
            "{}/api/v3/orgs/{}/repos",
            account.api_base.trim_end_matches('/'),
            account.owner
        );
        let resp = client
            .get(&url)
            .header("Authorization", format!("token {}", account.token))
            .send()?;
        if !resp.status().is_success() {
            return Err(anyhow!("Gitbucket list_repos failed: {}", resp.status()));
        }
        let repos: Vec<GhRepo> = resp.json()?;
        Ok(repos.into_iter().map(|r| r.name).collect())
    }

    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()> {
        let client = http_client();
        let url = format!(
            "{}/api/v3/orgs/{}/repos",
            account.api_base.trim_end_matches('/'),
            account.owner
        );
        let body = serde_json::json!({ "name": repo, "private": true });
        let resp = client
            .post(&url)
            .header("Authorization", format!("token {}", account.token))
            .json(&body)
            .send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("Gitbucket ensure_repo failed: {}", resp.status()))
        }
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        let base = account
            .git_base
            .trim_end_matches('/')
            .replacen("https://", "", 1)
            .replacen("http://", "", 1);
        format!(
            "https://{}:{}@{}/{}/{}.git",
            account.owner, account.token, base, account.owner, repo
        )
    }
}
