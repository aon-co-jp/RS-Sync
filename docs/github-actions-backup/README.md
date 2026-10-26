# GitHub Actions で自動バックアップ(GitHub → GitLab) / Automatic backup with GitHub Actions (GitHub → GitLab)

**日本語** / [English below](#english)

VPS やレンタルサーバーに何も置かず、**GitHub Actions だけ**で、GitHub のリポジトリを GitLab へ毎日自動で一方向コピー(バックアップ)するスクリプトと使い方です。
実際に動いている実装は [`open-mirror`](https://github.com/aon-co-jp/open-mirror)(公開)で、このフォルダの `mirror.sh` と `mirror.yml.example` はその写しです。更新の正本は `open-mirror` です。

## 何をコピーするか

- **対象**: 指定した GitHub アカウント(または組織)の**公開リポジトリ**。非公開リポジトリは対象外です(認証なしで読める範囲だけを使うため、PAT が要りません)。
- **コード**: ブランチ・タグ(強制上書き。GitLab 側で直接編集した内容は次回消えます)、PR の参照(`refs/github-pull/*`)。
- **Git LFS の実体**と **Wiki**(GitHub 側にある場合だけ)。
- **Releases**: GitLab の Release として作り直し、添付ファイルは GitLab の Generic Package Registry へ。古いリリースの添付は `RELEASE_KEEP`(例 `make-disk:3`)で最新N件に絞れます。
- **Issue・PR・コメント・レビュー・ラベル・マイルストーン**: JSON にして、GitLab の `github-metadata-backup` プロジェクトへ(`<リポジトリ名>/*.json`)。
- GitLab 側にプロジェクトが無ければ**非公開**で自動作成します。
- 対象外: Discussions、Projects、Actions の履歴・Secrets、Issue/PR に貼った画像の実体。

## 使い方(約10分)

1. **GitLab を用意**する。グループ(例 `my-backup-group`)を作り、`api` 権限の **Personal Access Token** を発行する。グループの数値 ID(グループのページに表示)も控える。
2. **GitHub にバックアップ用リポジトリを作る**(例 `mirror`)。**公開リポジトリにする**こと(下の「料金」参照)。
3. このフォルダの `mirror.sh` をリポジトリの直下に、`mirror.yml.example` を `.github/workflows/mirror.yml` として置く。
4. リポジトリの **Settings → Secrets and variables → Actions** に登録する。

   | 種類 | 名前 | 値 |
   |---|---|---|
   | Secret | `GITLAB_TOKEN` | 手順1のトークン |
   | Variable | `GITLAB_NAMESPACE_ID` | GitLab グループの数値 ID |
   | Variable | `GITLAB_NAMESPACE_PATH` | グループのパス(例 `my-backup-group`) |
   | Variable(任意) | `EXCLUDE_REPOS` | 除外するリポジトリ名(空白区切り) |
   | Variable(任意) | `RELEASE_KEEP` | 添付ファイルを残す最新リリース数(例 `big-app:3`) |

5. **Actions → Mirror to GitLab → Run workflow** で、まず `only_repos` に 1 つだけ入れて試す。成功したら空欄で全件。
6. 以後は毎日自動(`mirror.yml.example` の `cron`。既定は 19:00 UTC = 04:00 JST)。

ミラー元のアカウントは、実行するリポジトリの所有者が既定です(`GH_OWNER` で変更可)。

## 料金について(重要)

- GitHub Actions は、**公開リポジトリなら標準のランナーで実行時間が無料**です。
- **非公開リポジトリは月の無料枠(個人の Free で 2,000 分)を超えると有料**になり、支払いに失敗するとジョブが始まらなくなります(「The job was not started because recent account payments have failed…」)。
- 公開リポジトリにしても、トークンは Secrets に置かれ、実行ログでは `***` に伏せられます。ただし**ログ自体は公開**されます。

## 安全について

- トークンは必ず Secrets に置き、スクリプトやリポジトリには書かない。
- 一方向のバックアップです。GitLab 側を直接編集すると次回の同期で上書きされます。
- 復元は、GitLab のプロジェクトから普通に `git clone` するだけです。

## 制限

- 公開リポジトリだけが対象です。非公開もバックアップしたい場合は、読み取り権限付きの PAT を使うようにスクリプトを変える必要があります(このスクリプトはそうしていません)。
- GitLab の無料プランには容量などの制限があります。大きなリポジトリ・添付ファイルが多い場合は `EXCLUDE_REPOS` / `RELEASE_KEEP` で絞ってください。

---

<a id="english"></a>

# Automatic backup with GitHub Actions (GitHub → GitLab)

Scripts and instructions to copy your GitHub repositories to GitLab every day, one-way, using **only GitHub Actions** — nothing runs on your own server.
The implementation that actually runs is [`open-mirror`](https://github.com/aon-co-jp/open-mirror) (public); `mirror.sh` and `mirror.yml.example` here are copies. `open-mirror` is the source of truth.

## What is copied

- **Scope**: the **public repositories** of a GitHub account (or organization). Private repositories are not included — only data readable without authentication is used, so no PAT is needed.
- **Code**: branches and tags (force-updated; edits made directly on GitLab are overwritten next run) and pull-request refs (`refs/github-pull/*`).
- **Git LFS objects** and **wikis** (only where they exist on GitHub).
- **Releases**: recreated as GitLab Releases; assets go to GitLab's Generic Package Registry. Keep assets for only the newest N releases with `RELEASE_KEEP` (e.g. `make-disk:3`).
- **Issues, PRs, comments, reviews, labels, milestones**: saved as JSON in a GitLab project named `github-metadata-backup` (`<repo>/*.json`).
- Missing GitLab projects are created automatically as **private**.
- Not included: Discussions, Projects, Actions history/Secrets, images attached to issues/PRs.

## Setup (about 10 minutes)

1. **Prepare GitLab.** Create a group (e.g. `my-backup-group`) and a **Personal Access Token** with the `api` scope. Note the group's numeric ID (shown on the group page).
2. **Create a backup repository on GitHub** (e.g. `mirror`). **Make it public** (see "Billing" below).
3. Put `mirror.sh` at the repository root and `mirror.yml.example` at `.github/workflows/mirror.yml`.
4. Register in **Settings → Secrets and variables → Actions**:

   | Type | Name | Value |
   |---|---|---|
   | Secret | `GITLAB_TOKEN` | the token from step 1 |
   | Variable | `GITLAB_NAMESPACE_ID` | numeric ID of the GitLab group |
   | Variable | `GITLAB_NAMESPACE_PATH` | group path (e.g. `my-backup-group`) |
   | Variable (optional) | `EXCLUDE_REPOS` | repository names to skip (space-separated) |
   | Variable (optional) | `RELEASE_KEEP` | newest releases whose assets to keep (e.g. `big-app:3`) |

5. **Actions → Mirror to GitLab → Run workflow.** Try it first with a single repository in `only_repos`; leave it empty for all.
6. After that it runs daily (`cron` in `mirror.yml.example`; default 19:00 UTC = 04:00 JST).

The source account defaults to the owner of the repository that runs the workflow (override with `GH_OWNER`).

## Billing (important)

- GitHub Actions is **free for public repositories on standard runners**.
- **Private repositories are billed beyond the monthly free minutes** (2,000 min on a personal Free plan), and if a payment fails the job will not start ("The job was not started because recent account payments have failed…").
- Even in a public repository the token lives in Secrets and is masked (`***`) in logs, but **the logs themselves are public**.

## Safety

- Keep tokens in Secrets only; never in scripts or the repository.
- It is a one-way backup; direct edits on GitLab are overwritten by the next sync.
- To restore, simply `git clone` from the GitLab project.

## Limitations

- Public repositories only. To back up private ones too, the script would have to use a PAT with read access (it does not).
- GitLab's free plan has storage limits. For big repositories or many release assets, narrow things with `EXCLUDE_REPOS` / `RELEASE_KEEP`.
