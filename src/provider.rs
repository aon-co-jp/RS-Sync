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
        ProviderKind::GitLab => Box::new(GitLabProvider),
        ProviderKind::Bitbucket => Box::new(BitbucketProvider),
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

/// Adds an `Authorization`-style header only when `token` is non-empty.
///
/// **Real bug fixed 2026-07-29**: accounts with an empty token (documented
/// in the UI as valid for "public repos only") were still sending a
/// malformed header like `Authorization: token ` (empty value) on every
/// request. GitHub's API treats that as an actual failed authentication
/// attempt and returns `401 Unauthorized` — it does **not** fall back to
/// anonymous access the way omitting the header entirely does. Found via
/// a real quick-setup job that failed with "repo discovery failed: GitHub
/// list_repos failed: 401 Unauthorized" despite the account being valid
/// for anonymous/public access.
fn maybe_auth_header(
    req: reqwest::blocking::RequestBuilder,
    header_name: &str,
    token: &str,
    format_value: impl Fn(&str) -> String,
) -> reqwest::blocking::RequestBuilder {
    if token.is_empty() {
        req
    } else {
        req.header(header_name, format_value(token))
    }
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
            let resp = maybe_auth_header(client.get(&url), "Authorization", &account.token, |t| format!("token {t}"))
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
                maybe_auth_header(client.get(&user_url), "Authorization", &account.token, |t| format!("token {t}"))
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
        let resp = maybe_auth_header(client.get(&check_url), "Authorization", &account.token, |t| format!("token {t}"))
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
        let resp = maybe_auth_header(client.post(&create_url), "Authorization", &account.token, |t| format!("token {t}"))
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()?;
        if resp.status().is_success() {
            return Ok(());
        }
        // Retry against /user/repos in case owner is a personal account, not an org.
        let user_create_url = format!("{}/user/repos", account.api_base.trim_end_matches('/'));
        let resp = maybe_auth_header(client.post(&user_create_url), "Authorization", &account.token, |t| format!("token {t}"))
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
        // https://<token>@github.com/<owner>/<repo>.git — omit the
        // "<token>@" prefix entirely when the token is empty (anonymous
        // clone of a public repo); a literal empty username before `@`
        // confuses some git/HTTP client combinations.
        let base = account
            .git_base
            .trim_end_matches('/')
            .replacen("https://", "", 1)
            .replacen("http://", "", 1);
        if account.token.is_empty() {
            format!("https://{}/{}/{}.git", base, account.owner, repo)
        } else {
            format!("https://{}@{}/{}/{}.git", account.token, base, account.owner, repo)
        }
    }
}

// ---------------- open-gitea (self-hosted) ----------------
// open-gitea (aon-co-jp/open-gitea) exposes git smart-HTTP directly plus a small JSON API
// (`GET /api/repos`, `PUT /repos/:name` to create). Repo creation and git push both
// require auth: open-gitea accepts `Authorization: Bearer <session-token>` on the
// JSON API, and `email:session-token` Basic auth embedded in the git remote URL
// (see open-gitea's CLAUDE.md "git CLI向け認証方式"). `account.owner` holds the
// login email for this provider (not an org/namespace like GitHub/Gitea), and
// `account.token` holds the OTP session token obtained via
// `POST /api/auth/verify-otp` on the open-gitea instance.
pub struct OpenGiteaProvider;

#[derive(Deserialize)]
struct OpenGiteaRepoEntry {
    name: String,
}

impl Provider for OpenGiteaProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let client = http_client();
        let url = format!("{}/api/repos", account.api_base.trim_end_matches('/'));
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", account.token))
            .send()?;
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
        let resp = client
            .put(&url)
            .header("Authorization", format!("Bearer {}", account.token))
            .send()?;
        // open-gitea returns success or "already exists" style responses; treat any
        // non-5xx as acceptable since this is best-effort per the docs above.
        if resp.status().is_server_error() {
            return Err(anyhow!("open-gitea ensure_repo failed: {}", resp.status()));
        }
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(anyhow!(
                "open-gitea ensure_repo failed: {} (check account token/permissions)",
                resp.status()
            ));
        }
        Ok(())
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        // https://<email>:<session-token>@<host>/<repo>.git — open-gitea repos are
        // not namespaced by owner/org, so no owner segment in the path.
        let base = account
            .git_base
            .trim_end_matches('/')
            .replacen("https://", "", 1)
            .replacen("http://", "", 1);
        format!(
            "https://{}:{}@{}/{}.git",
            account.owner, account.token, base, repo
        )
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

// ---------------- GitLab (documented, not verified live) ----------------
// GitLab REST API v4 (https://docs.gitlab.com/ee/api/projects.html) uses a
// `PRIVATE-TOKEN` header and a personal-namespace project list under
// `/api/v4/users/:username/projects` (works for both gitlab.com and
// self-hosted instances). Not yet exercised against a live GitLab instance;
// documented here honestly as unverified, mirroring the existing
// Gitea/GitBucket adapters' disclosure pattern.
pub struct GitLabProvider;

#[derive(Deserialize)]
struct GitLabProject {
    path: String,
}

impl Provider for GitLabProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let client = http_client();
        let url = format!(
            "{}/api/v4/users/{}/projects?per_page=100",
            account.api_base.trim_end_matches('/'),
            account.owner
        );
        let resp = client.get(&url).header("PRIVATE-TOKEN", &account.token).send()?;
        if !resp.status().is_success() {
            return Err(anyhow!("GitLab list_repos failed: {}", resp.status()));
        }
        let projects: Vec<GitLabProject> = resp.json()?;
        Ok(projects.into_iter().map(|p| p.path).collect())
    }

    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()> {
        let client = http_client();
        let check_url = format!(
            "{}/api/v4/projects/{}",
            account.api_base.trim_end_matches('/'),
            urlencode(&format!("{}/{}", account.owner, repo))
        );
        let resp = client.get(&check_url).header("PRIVATE-TOKEN", &account.token).send()?;
        if resp.status().is_success() {
            return Ok(());
        }
        let create_url = format!("{}/api/v4/projects", account.api_base.trim_end_matches('/'));
        let body = serde_json::json!({ "name": repo, "visibility": "private" });
        let resp = client.post(&create_url).header("PRIVATE-TOKEN", &account.token).json(&body).send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("GitLab ensure_repo failed: {}", resp.status()))
        }
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        // https://oauth2:<personal-access-token>@<host>/<owner>/<repo>.git
        // ("oauth2" as the literal username is GitLab's documented
        // convention for authenticating over HTTPS with a PAT).
        let base = account
            .git_base
            .trim_end_matches('/')
            .replacen("https://", "", 1)
            .replacen("http://", "", 1);
        format!("https://oauth2:{}@{}/{}/{}.git", account.token, base, account.owner, repo)
    }
}

// ---------------- Bitbucket (Atlassian Cloud, documented, not verified live) ----------------
// Bitbucket Cloud REST API v2 (https://developer.atlassian.com/cloud/bitbucket/rest/).
// Distinct from the self-hosted "GitBucket" OSS project (see
// [`GitbucketProvider`] above) despite the similar name. Uses a repository
// access token as a Bearer token. Not yet exercised against a live
// Bitbucket workspace; documented here honestly as unverified, mirroring
// the existing Gitea/GitBucket/GitLab adapters' disclosure pattern. **Only
// the first page of repos is listed** (no pagination via the `next`
// field) — acceptable for a handful of repos per workspace, a known
// limitation for larger workspaces.
pub struct BitbucketProvider;

#[derive(Deserialize)]
struct BitbucketRepoList {
    values: Vec<BitbucketRepo>,
}

#[derive(Deserialize)]
struct BitbucketRepo {
    slug: String,
}

impl Provider for BitbucketProvider {
    fn list_repos(&self, account: &Account) -> Result<Vec<String>> {
        let client = http_client();
        let url = format!(
            "{}/2.0/repositories/{}?pagelen=100",
            account.api_base.trim_end_matches('/'),
            account.owner
        );
        let resp = client.get(&url).header("Authorization", format!("Bearer {}", account.token)).send()?;
        if !resp.status().is_success() {
            return Err(anyhow!("Bitbucket list_repos failed: {}", resp.status()));
        }
        let list: BitbucketRepoList = resp.json()?;
        Ok(list.values.into_iter().map(|r| r.slug).collect())
    }

    fn ensure_repo(&self, account: &Account, repo: &str) -> Result<()> {
        let client = http_client();
        let url = format!(
            "{}/2.0/repositories/{}/{}",
            account.api_base.trim_end_matches('/'),
            account.owner,
            repo
        );
        let body = serde_json::json!({ "scm": "git", "is_private": true });
        let resp = client.post(&url).header("Authorization", format!("Bearer {}", account.token)).json(&body).send()?;
        if resp.status().is_success() || resp.status() == reqwest::StatusCode::CONFLICT {
            Ok(())
        } else {
            Err(anyhow!("Bitbucket ensure_repo failed: {}", resp.status()))
        }
    }

    fn remote_url(&self, account: &Account, repo: &str) -> String {
        // https://x-token-auth:<repository-access-token>@bitbucket.org/<owner>/<repo>.git
        // ("x-token-auth" as the literal username is Bitbucket's documented
        // convention for authenticating with a repository access token).
        let base = account
            .git_base
            .trim_end_matches('/')
            .replacen("https://", "", 1)
            .replacen("http://", "", 1);
        format!("https://x-token-auth:{}@{}/{}/{}.git", account.token, base, account.owner, repo)
    }
}

/// Minimal path-segment percent-encoding (no new dependency), sufficient
/// for GitLab's `namespace%2Fproject` URL-encoded project path convention.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderKind;

    fn account(token: &str) -> Account {
        Account {
            id: "1".to_string(),
            name: "test".to_string(),
            provider: ProviderKind::GitHub,
            api_base: "https://api.github.com".to_string(),
            git_base: "https://github.com".to_string(),
            owner: "aon-co-jp".to_string(),
            token: token.to_string(),
        }
    }

    /// 2026-07-29発見の実バグの回帰確認: 空トークンのアカウントで
    /// `remote_url`が生成するgit URLに、空のユーザー名+`@`が残っていない
    /// ことを確認する(残っていると一部のgit/HTTPクライアントの組み合わせで
    /// 混乱を招く)。
    #[test]
    fn github_remote_url_omits_empty_token_prefix() {
        let provider = GitHubProvider;
        let url = provider.remote_url(&account(""), "myrepo");
        assert_eq!(url, "https://github.com/aon-co-jp/myrepo.git");
    }

    #[test]
    fn github_remote_url_embeds_nonempty_token() {
        let provider = GitHubProvider;
        let url = provider.remote_url(&account("ghp_secret"), "myrepo");
        assert_eq!(url, "https://ghp_secret@github.com/aon-co-jp/myrepo.git");
    }

    /// 2026-07-29発見の実バグの回帰確認: 空トークンの場合、
    /// `Authorization`ヘッダ自体を送らないことを確認する(送ると
    /// GitHub APIが匿名アクセスにフォールバックせず401を返してしまう
    /// ——実際にクイック設定GUIで空トークンのアカウントを使った際に
    /// 発生した実障害)。
    #[test]
    fn maybe_auth_header_skips_header_when_token_is_empty() {
        let client = http_client();
        let req = maybe_auth_header(client.get("https://example.invalid"), "Authorization", "", |t| format!("token {t}"));
        let built = req.build().unwrap();
        assert!(built.headers().get("Authorization").is_none());
    }

    #[test]
    fn maybe_auth_header_adds_header_when_token_is_present() {
        let client = http_client();
        let req = maybe_auth_header(client.get("https://example.invalid"), "Authorization", "secret", |t| format!("token {t}"));
        let built = req.build().unwrap();
        assert_eq!(built.headers().get("Authorization").unwrap(), "token secret");
    }
}
