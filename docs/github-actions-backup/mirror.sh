#!/usr/bin/env bash
# GitHub (aon-co-jp) → GitLab (aon-co-jp-group) 一方向バックアップ
#   - コード: ブランチ・タグ・PRのref(refs/github-pull/*)
#   - Git LFS実体 / Wiki
#   - Releases: GitLabのReleaseとして再作成、添付ファイルはGeneric Package Registryへ
#   - Issue / PR / コメント / レビュー / リポジトリ情報: JSONとしてメタデータ用プロジェクトへ
# 必要な環境変数:
#   GH_TOKEN               Actions標準のgithub.token(公開リポジトリの読み取りのみ。APIレート制限緩和用で、PATは不要)
#   GITLAB_TOKEN           GitLab PAT (api)
#   GITLAB_NAMESPACE_ID    例: 143088430
#   GITLAB_NAMESPACE_PATH  例: aon-co-jp-group
# 任意:
#   EXCLUDE_REPOS          除外するリポジトリ名(空白区切り)
#   ONLY_REPOS             指定時はこのリポジトリだけ処理(空白区切り、テスト用)
#   RELEASE_KEEP           添付ファイルを保持する最新リリース数(例: "make-disk:3 foo:1")。既定 make-disk:3、記載の無いリポジトリは全件
#   METADATA_PROJECT       メタデータ保存先プロジェクト名(既定: github-metadata-backup)
set -uo pipefail

: "${GH_TOKEN:?}" "${GITLAB_TOKEN:?}" "${GITLAB_NAMESPACE_ID:?}" "${GITLAB_NAMESPACE_PATH:?}"
GH_OWNER="${GH_OWNER:-aon-co-jp}" # ミラー元のGitHubアカウント/組織名(既定はこのリポジトリの所有者向けの値)
GL_API="https://gitlab.com/api/v4"
RELEASE_KEEP="${RELEASE_KEEP:-make-disk:3}"
META_PROJECT="${METADATA_PROJECT:-github-metadata-backup}"
PKG="github-release"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export GIT_TERMINAL_PROMPT=0
GH_URL="https://github.com/$GH_OWNER" # 公開リポジトリのみ対象なので認証なし
GL_URL="https://oauth2:${GITLAB_TOKEN}@gitlab.com/$GITLAB_NAMESPACE_PATH"

gl() { # gl METHOD PATH [curl args...]
  local m="$1" p="$2"; shift 2
  curl -sS -X "$m" -H "PRIVATE-TOKEN: $GITLAB_TOKEN" "$@" "$GL_API$p"
}
urlenc() { jq -rn --arg s "$1" '$s|@uri'; }
META_ERR=0
ghall() { # 全ページを1配列に。取得失敗(権限不足等)はMETA_ERRに記録
  local out
  if ! out="$(gh api --paginate "$1" 2>&1)"; then
    echo "::warning::取得失敗: $1 ($(head -c 200 <<<"$out"))" >&2; META_ERR=1; echo "[]"; return 1
  fi
  jq -s 'add // []' <<<"$out"
}
release_keep() { local kv; for kv in ${RELEASE_KEEP:-}; do [[ "${kv%%:*}" == "$1" ]] && { echo "${kv#*:}"; return; }; done; echo 0; }

gl_project() { # gl_project NAME DESC → project id (無ければprivateで作成)
  local id
  id="$(gl GET "/projects/$(urlenc "$GITLAB_NAMESPACE_PATH/$1")" | jq -r '.id // empty')"
  if [[ -z "$id" ]]; then
    echo "GitLabにプロジェクトを作成(private): $1" >&2
    id="$(gl POST /projects \
      --data-urlencode "name=$1" --data-urlencode "path=$1" \
      --data-urlencode "namespace_id=$GITLAB_NAMESPACE_ID" \
      --data-urlencode "visibility=private" \
      --data-urlencode "description=$2" | jq -r '.id // empty')"
  fi
  echo "$id"
}

push_retry() { # push_retry GITDIR URL REFSPEC...
  local dir="$1" url="$2" wait; shift 2
  # 作成直後のプロジェクト等でGitLabが一時的に422/5xxを返すことがあるため再試行
  for wait in 0 20 60; do
    sleep "$wait"
    git -C "$dir" push --quiet --prune "$url" "$@" 2>&1 && return 0
    echo "::warning::push失敗、再試行します" >&2
  done
  return 1
}

# ---- Releases ----
backup_releases() { # NAME PID RELEASES_JSON
  local name="$1" pid="$2" rel="$3" keep tag i=0 rc=0
  keep="$(release_keep "$name")"
  local kept_tags=() pkgs
  while IFS= read -r r; do
    tag="$(jq -r .tag_name <<<"$r")"; i=$((i+1))
    local etag; etag="$(urlenc "$tag")"
    # GitLabのReleaseを作成(タグはコード同期で既に存在)
    if [[ "$(gl GET "/projects/$pid/releases/$etag" -o /dev/null -w '%{http_code}')" != 200 ]]; then
      jq -n --argjson r "$r" '{tag_name:$r.tag_name, name:($r.name // $r.tag_name),
          description:(($r.body // "") + "\n\n---\n[mirror of GitHub release " + $r.html_url + " / published " + ($r.published_at // "") + "]")}' \
        | gl POST "/projects/$pid/releases" -H "Content-Type: application/json" -d @- -o /dev/null \
        || { echo "::error::$name: release $tag 作成失敗"; rc=1; continue; }
    fi
    local links; links="$(gl GET "/projects/$pid/releases/$etag/assets/links?per_page=100")"
    if [[ "$keep" -ne 0 && "$i" -gt "$keep" ]]; then
      # 保持数を超えた古いリリース: 添付ファイルへのリンクを外す(パッケージは後でまとめて削除)
      jq -r '.[]? | select(.url|contains("/packages/generic/'"$PKG"'/")) | .id' <<<"$links" | while read -r lid; do
        gl DELETE "/projects/$pid/releases/$etag/assets/links/$lid" -o /dev/null
      done
      continue
    fi
    kept_tags+=("$tag")
    # このバージョンで既にアップロード済みのファイル名一覧
    local have="" pkgid
    pkgid="$(gl GET "/projects/$pid/packages?package_type=generic&package_name=$PKG&package_version=$etag" | jq -r '(arrays | .[0].id) // empty')"
    [[ -n "$pkgid" ]] && have="$(gl GET "/projects/$pid/packages/$pkgid/package_files?per_page=100" | jq -r '.[]?.file_name')"
    while IFS=$'\t' read -r aname aurl; do
      [[ -z "$aname" ]] && continue
      local purl="$GL_API/projects/$pid/packages/generic/$PKG/$etag/$(urlenc "$aname")"
      if ! grep -qxF -- "$aname" <<<"$have"; then
        echo "添付ファイル転送: $tag/$aname"
        if curl -sSfL -H "Accept: application/octet-stream" "$aurl" -o "$WORK/asset" \
           && curl -sSf -H "PRIVATE-TOKEN: $GITLAB_TOKEN" --upload-file "$WORK/asset" "$purl?select=package_file" -o /dev/null; then :; else
          echo "::error::$name: $tag/$aname 転送失敗"; rc=1; rm -f "$WORK/asset"; continue
        fi
        rm -f "$WORK/asset"
      fi
      if ! jq -e --arg u "$purl" 'any(.[]?; .url==$u)' <<<"$links" >/dev/null; then
        gl POST "/projects/$pid/releases/$etag/assets/links" --data-urlencode "name=$aname" \
          --data-urlencode "url=$purl" --data-urlencode "link_type=package" -o /dev/null
      fi
    done < <(jq -r '.assets[] | [.name, .url] | @tsv' <<<"$r")
  done < <(jq -c '.[] | select(.draft|not)' <<<"$rel")

  # 保持対象外になったバージョンのパッケージを削除(容量節約)
  if [[ "$keep" -ne 0 ]]; then
    pkgs="$(gl GET "/projects/$pid/packages?package_type=generic&package_name=$PKG&per_page=100")"
    jq -r '.[]? | [.id, .version] | @tsv' <<<"$pkgs" | while IFS=$'\t' read -r id ver; do
      [[ " ${kept_tags[*]} " == *" $ver "* ]] && continue
      echo "古い添付ファイルを削除: $ver"; gl DELETE "/projects/$pid/packages/$id" -o /dev/null
    done
  fi
  return $rc
}

# ---- Issue / PR 等のメタデータ ----
backup_metadata() { # NAME RELEASES_JSON
  local name="$1" d="$WORK/meta/$1"
  META_ERR=0
  mkdir -p "$d"
  gh api "repos/$GH_OWNER/$name" > "$d/repo.json" 2>/dev/null || return 1
  if jq -e '.has_issues' "$d/repo.json" >/dev/null; then # Issue無効のリポジトリは410になるため除外
    ghall "repos/$GH_OWNER/$name/issues?state=all&per_page=100"        > "$d/issues.json"   # PRも含む
    ghall "repos/$GH_OWNER/$name/issues/comments?per_page=100"         > "$d/issue_comments.json"
  fi
  ghall "repos/$GH_OWNER/$name/pulls?state=all&per_page=100"           > "$d/pulls.json"
  ghall "repos/$GH_OWNER/$name/pulls/comments?per_page=100"            > "$d/pull_review_comments.json"
  ghall "repos/$GH_OWNER/$name/labels?per_page=100"                    > "$d/labels.json"
  ghall "repos/$GH_OWNER/$name/milestones?state=all&per_page=100"      > "$d/milestones.json"
  printf '%s' "$2"                                                      > "$d/releases.json"
  local reviews="[]" n
  for n in $(jq -r '.[].number' "$d/pulls.json"); do
    ghall "repos/$GH_OWNER/$name/pulls/$n/reviews?per_page=100" > "$WORK/rv.json"
    reviews="$(jq --argjson n "$n" --slurpfile r "$WORK/rv.json" '. + [{pull:$n, reviews:$r[0]}]' <<<"$reviews")"
  done
  printf '%s' "$reviews" > "$d/pull_reviews.json"
  # 変化の無い日に差分が出ないよう整形・ソート
  for f in "$d"/*.json; do jq -S . "$f" > "$f.tmp" && mv "$f.tmp" "$f"; done
  [[ $META_ERR -eq 0 ]]
}

# ---- メタデータ用プロジェクトの準備 ----
META_PID="$(gl_project "$META_PROJECT" "GitHub aon-co-jp の Issue/PR/Release 等のJSONバックアップ(自動生成)")"
mkdir -p "$WORK/meta"
git clone --quiet "$GL_URL/$META_PROJECT.git" "$WORK/meta" 2>/dev/null || git -C "$WORK/meta" init --quiet -b main

mapfile -t REPOS < <(gh api --paginate "users/$GH_OWNER/repos?per_page=100" \
  --jq '.[] | "\(.name)\t\(.description // "")"')

ok=0; skip=0; failed=()
for line in "${REPOS[@]}"; do
  IFS=$'\t' read -r name desc <<<"$line"
  if [[ -n "${ONLY_REPOS:-}" && " $ONLY_REPOS " != *" $name "* ]]; then continue; fi
  if [[ " ${EXCLUDE_REPOS:-} " == *" $name "* ]]; then echo "::notice::skip $name"; skip=$((skip+1)); continue; fi
  [[ "$name" == "$META_PROJECT" ]] && { echo "::error::$name はメタデータ用プロジェクト名と衝突"; failed+=("$name"); continue; }

  echo "::group::$name"
  errs=()
  pid="$(gl_project "$name" "[mirror of github.com/$GH_OWNER/$name] $desc")"
  if [[ -z "$pid" ]]; then echo "::error::$name: プロジェクト作成失敗"; failed+=("$name(project)"); echo "::endgroup::"; continue; fi

  # 一方向ミラーなので、強制pushを拒否する保護ブランチを外す
  for b in $(gl GET "/projects/$pid/protected_branches?per_page=100" | jq -r '.[]?.name'); do
    gl DELETE "/projects/$pid/protected_branches/$(urlenc "$b")" -o /dev/null
  done

  # コード(ブランチ・タグ・PRのref)
  dir="$WORK/$name.git"
  if git clone --quiet --mirror "$GH_URL/$name.git" "$dir" 2>&1; then
    push_retry "$dir" "$GL_URL/$name.git" \
      '+refs/heads/*:refs/heads/*' '+refs/tags/*:refs/tags/*' '+refs/pull/*:refs/github-pull/*' || errs+=(code)
    # Git LFS
    if git -C "$dir" show HEAD:.gitattributes 2>/dev/null | grep -q 'filter=lfs'; then
      echo "Git LFS実体を転送"
      { git -C "$dir" lfs fetch --all origin && git -C "$dir" lfs push --all "$GL_URL/$name.git"; } 2>&1 || errs+=(lfs)
    fi
  else
    errs+=(clone)
  fi
  rm -rf "$dir"

  # Wiki(GitHub側に実体がある場合のみ)
  if git ls-remote "$GH_URL/$name.wiki.git" >/dev/null 2>&1; then
    echo "Wikiを同期"
    wdir="$WORK/$name.wiki.git"
    { git clone --quiet --mirror "$GH_URL/$name.wiki.git" "$wdir" \
      && push_retry "$wdir" "$GL_URL/$name.wiki.git" '+refs/heads/*:refs/heads/*'; } 2>&1 || errs+=(wiki)
    rm -rf "$wdir"
  fi

  # Releases と メタデータ
  rel="$(ghall "repos/$GH_OWNER/$name/releases?per_page=100")"
  backup_releases "$name" "$pid" "$rel" || errs+=(releases)
  backup_metadata "$name" "$rel" || errs+=(metadata)

  if [[ ${#errs[@]} -eq 0 ]]; then ok=$((ok+1)); else
    echo "::error::$name: 失敗 (${errs[*]})"; failed+=("$name(${errs[*]})")
  fi
  echo "::endgroup::"
done

# メタデータをコミットしてpush(変化がある時だけ)
cd "$WORK/meta"
git add -A
if ! git diff --cached --quiet; then
  git -c user.name="gitlab-mirror" -c user.email="gitlab-mirror@users.noreply.github.com" \
    commit --quiet -m "GitHubメタデータ同期 $(date -u +%Y-%m-%dT%H:%MZ)"
  git push --quiet "$GL_URL/$META_PROJECT.git" HEAD:main 2>&1 || failed+=("$META_PROJECT(push)")
fi
cd - >/dev/null

{
  echo "## GitLabバックアップ結果"
  echo "- 成功: $ok / 除外: $skip / 失敗: ${#failed[@]}"
  for f in "${failed[@]}"; do echo "  - ❌ $f"; done
} | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"

[[ ${#failed[@]} -eq 0 ]]
