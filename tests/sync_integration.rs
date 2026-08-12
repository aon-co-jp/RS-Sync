use rs_sync::model::{Account, Direction, ProviderKind, Schedule, SyncJob};
use rs_sync::sync_engine::run_job;
use std::process::Command;
use uuid::Uuid;

fn git(args: &[&str], cwd: &std::path::Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("run git");
    assert!(status.success(), "git {:?} failed", args);
}

fn make_account(name: &str, dir: &std::path::Path) -> Account {
    Account {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        provider: ProviderKind::Local,
        api_base: String::new(),
        git_base: dir.to_str().unwrap().to_string(),
        owner: "test".to_string(),
        token: String::new(),
        token_expires_at: None,
        expiry_warning_sent: false,
    }
}

fn empty_schedule() -> Schedule {
    Schedule {
        every_hours: None,
        days_of_week: vec![],
        hour: None,
        minute: None,
        realtime_poll_minutes: None,
    }
}

/// Full one-way sync: create a bare "source" repo, push a commit into it via
/// a working clone, run a one-way job, and assert the commit shows up in the
/// bare "dest" repo.
#[test]
fn one_way_sync_replicates_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let job_root = tmp.path().join(format!("job-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&job_root).unwrap();
    let source_dir = job_root.join("source_provider");
    let dest_dir = job_root.join("dest_provider");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();

    let src_bare = source_dir.join("myrepo.git");
    git(&["init", "--bare", src_bare.to_str().unwrap()], &job_root);

    // Populate the source with a commit via a working clone.
    let work = job_root.join("work");
    git(&["clone", src_bare.to_str().unwrap(), work.to_str().unwrap()], &job_root);
    std::fs::write(work.join("hello.txt"), "hello from rs-sync test\n").unwrap();
    git(&["-c", "user.email=test@example.com", "-c", "user.name=Test", "add", "hello.txt"], &work);
    git(&["-c", "user.email=test@example.com", "-c", "user.name=Test", "commit", "-m", "init"], &work);
    git(&["push", "origin", "HEAD:refs/heads/main"], &work);

    let source = make_account("source", &source_dir);
    let dest = make_account("dest", &dest_dir);

    // rs-sync's own cache dirs go under a scratch data dir for this test run.
    std::env::set_current_dir(&job_root).unwrap();

    let mut job = SyncJob {
        id: Uuid::new_v4().to_string(),
        name: "test-one-way".to_string(),
        source_account_id: source.id.clone(),
        dest_account_id: dest.id.clone(),
        direction: Direction::OneWay,
        repos: vec!["myrepo".to_string()],
        schedule: empty_schedule(),
        enabled: true,
        last_run_at: None,
        last_run_status: None,
        conflicted_repos: vec![],
        initial_realtime_poll_minutes: None,
    };

    let outcome = run_job(&mut job, &source, &dest);
    assert!(outcome.success, "sync failed: {}", outcome.message);

    // Verify the destination bare repo now has the commit.
    let dst_bare = dest_dir.join("myrepo.git");
    assert!(dst_bare.exists(), "destination repo was not created");
    let check = job_root.join("check");
    git(&["clone", dst_bare.to_str().unwrap(), check.to_str().unwrap()], &job_root);
    git(&["checkout", "main"], &check);
    let content = std::fs::read_to_string(check.join("hello.txt")).unwrap();
    assert_eq!(content, "hello from rs-sync test\n");
}

/// Two-way sync: change on the source side only should propagate to dest,
/// with no conflict flagged.
#[test]
fn two_way_sync_propagates_source_change_without_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let job_root = tmp.path().join(format!("job2-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&job_root).unwrap();
    let source_dir = job_root.join("source_provider");
    let dest_dir = job_root.join("dest_provider");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();

    let src_bare = source_dir.join("repo2.git");
    git(&["init", "--bare", src_bare.to_str().unwrap()], &job_root);

    let work = job_root.join("work2");
    git(&["clone", src_bare.to_str().unwrap(), work.to_str().unwrap()], &job_root);
    std::fs::write(work.join("a.txt"), "v1\n").unwrap();
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "add", "a.txt"], &work);
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "commit", "-m", "v1"], &work);
    git(&["push", "origin", "HEAD:refs/heads/main"], &work);

    let source = make_account("source2", &source_dir);
    let dest = make_account("dest2", &dest_dir);
    std::env::set_current_dir(&job_root).unwrap();

    let mut job = SyncJob {
        id: Uuid::new_v4().to_string(),
        name: "test-two-way".to_string(),
        source_account_id: source.id.clone(),
        dest_account_id: dest.id.clone(),
        direction: Direction::TwoWay,
        repos: vec!["repo2".to_string()],
        schedule: empty_schedule(),
        enabled: true,
        last_run_at: None,
        last_run_status: None,
        conflicted_repos: vec![],
        initial_realtime_poll_minutes: None,
    };

    let first = run_job(&mut job, &source, &dest);
    assert!(first.success, "first two-way sync failed: {}", first.message);
    assert!(job.conflicted_repos.is_empty());

    let dst_bare = dest_dir.join("repo2.git");
    let check = job_root.join("check2");
    git(&["clone", dst_bare.to_str().unwrap(), check.to_str().unwrap()], &job_root);
    git(&["checkout", "main"], &check);
    assert_eq!(std::fs::read_to_string(check.join("a.txt")).unwrap(), "v1\n");
}

/// Two-way sync: a change made only on the *dest* side (after an initial
/// sync has established a baseline) should propagate back to the source,
/// with no conflict flagged. This is the mirror-image of
/// `two_way_sync_propagates_source_change_without_conflict` and exercises
/// the `dst_changed` branch of `run_job`, which the existing test suite
/// never touched.
#[test]
fn two_way_sync_propagates_dest_only_change_back_to_source() {
    let tmp = tempfile::tempdir().unwrap();
    let job_root = tmp.path().join(format!("job3-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&job_root).unwrap();
    let source_dir = job_root.join("source_provider");
    let dest_dir = job_root.join("dest_provider");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();

    let src_bare = source_dir.join("repo3.git");
    git(&["init", "--bare", src_bare.to_str().unwrap()], &job_root);

    let work = job_root.join("work3");
    git(&["clone", src_bare.to_str().unwrap(), work.to_str().unwrap()], &job_root);
    std::fs::write(work.join("a.txt"), "v1\n").unwrap();
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "add", "a.txt"], &work);
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "commit", "-m", "v1"], &work);
    git(&["push", "origin", "HEAD:refs/heads/main"], &work);

    let source = make_account("source3", &source_dir);
    let dest = make_account("dest3", &dest_dir);
    std::env::set_current_dir(&job_root).unwrap();

    let mut job = SyncJob {
        id: Uuid::new_v4().to_string(),
        name: "test-two-way-dest-first".to_string(),
        source_account_id: source.id.clone(),
        dest_account_id: dest.id.clone(),
        direction: Direction::TwoWay,
        repos: vec!["repo3".to_string()],
        schedule: empty_schedule(),
        enabled: true,
        last_run_at: None,
        last_run_status: None,
        conflicted_repos: vec![],
        initial_realtime_poll_minutes: None,
    };

    // Establish a baseline: source has v1, nothing on dest yet.
    let first = run_job(&mut job, &source, &dest);
    assert!(first.success, "baseline sync failed: {}", first.message);
    assert!(job.conflicted_repos.is_empty());

    // Now change *only* the dest side.
    let dst_bare = dest_dir.join("repo3.git");
    let dst_work = job_root.join("dst_work3");
    git(&["clone", dst_bare.to_str().unwrap(), dst_work.to_str().unwrap()], &job_root);
    git(&["checkout", "main"], &dst_work);
    std::fs::write(dst_work.join("b.txt"), "from dest\n").unwrap();
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "add", "b.txt"], &dst_work);
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "commit", "-m", "dest change"], &dst_work);
    git(&["push", "origin", "HEAD:refs/heads/main"], &dst_work);

    let second = run_job(&mut job, &source, &dest);
    assert!(second.success, "propagation sync failed: {}", second.message);
    assert!(job.conflicted_repos.is_empty(), "an untouched-source, dest-only change must never be flagged as a conflict");

    // The source bare repo must now have the dest's new file.
    let check = job_root.join("check3");
    git(&["clone", src_bare.to_str().unwrap(), check.to_str().unwrap()], &job_root);
    git(&["checkout", "main"], &check);
    assert_eq!(std::fs::read_to_string(check.join("a.txt")).unwrap(), "v1\n");
    assert_eq!(std::fs::read_to_string(check.join("b.txt")).unwrap(), "from dest\n");
}

/// Two-way sync: when *both* sides are changed independently since the last
/// successful sync, `run_job` must detect a genuine conflict, record the repo
/// in `conflicted_repos`, and refuse to overwrite either side. A subsequent
/// run must then skip that repo entirely (manual resolution required) rather
/// than silently retrying. This is the safety-critical path documented in
/// CLAUDE.md ("force pushでの上書きはしない") that had no test coverage at all.
#[test]
fn two_way_sync_detects_conflict_when_both_sides_diverge_and_then_skips_it() {
    let tmp = tempfile::tempdir().unwrap();
    let job_root = tmp.path().join(format!("job4-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&job_root).unwrap();
    let source_dir = job_root.join("source_provider");
    let dest_dir = job_root.join("dest_provider");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();

    let src_bare = source_dir.join("repo4.git");
    git(&["init", "--bare", src_bare.to_str().unwrap()], &job_root);

    let work = job_root.join("work4");
    git(&["clone", src_bare.to_str().unwrap(), work.to_str().unwrap()], &job_root);
    std::fs::write(work.join("a.txt"), "v1\n").unwrap();
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "add", "a.txt"], &work);
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "commit", "-m", "v1"], &work);
    git(&["push", "origin", "HEAD:refs/heads/main"], &work);

    let source = make_account("source4", &source_dir);
    let dest = make_account("dest4", &dest_dir);
    std::env::set_current_dir(&job_root).unwrap();

    let mut job = SyncJob {
        id: Uuid::new_v4().to_string(),
        name: "test-two-way-conflict".to_string(),
        source_account_id: source.id.clone(),
        dest_account_id: dest.id.clone(),
        direction: Direction::TwoWay,
        repos: vec!["repo4".to_string()],
        schedule: empty_schedule(),
        enabled: true,
        last_run_at: None,
        last_run_status: None,
        conflicted_repos: vec![],
        initial_realtime_poll_minutes: None,
    };

    // Baseline sync establishes last_synced = v1 on both sides.
    let baseline = run_job(&mut job, &source, &dest);
    assert!(baseline.success, "baseline sync failed: {}", baseline.message);
    assert!(job.conflicted_repos.is_empty());

    // Diverge *both* sides independently from the v1 baseline.
    std::fs::write(work.join("a.txt"), "source-side-change\n").unwrap();
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "add", "a.txt"], &work);
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "commit", "-m", "source diverges"], &work);
    git(&["push", "origin", "HEAD:refs/heads/main"], &work);

    let dst_bare = dest_dir.join("repo4.git");
    let dst_work = job_root.join("dst_work4");
    git(&["clone", dst_bare.to_str().unwrap(), dst_work.to_str().unwrap()], &job_root);
    git(&["checkout", "main"], &dst_work);
    std::fs::write(dst_work.join("a.txt"), "dest-side-change\n").unwrap();
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "add", "a.txt"], &dst_work);
    git(&["-c", "user.email=t@e.com", "-c", "user.name=T", "commit", "-m", "dest diverges"], &dst_work);
    git(&["push", "origin", "HEAD:refs/heads/main"], &dst_work);

    let conflicting_run = run_job(&mut job, &source, &dest);
    assert!(conflicting_run.success, "run_job itself must not fail on conflict, only flag it: {}", conflicting_run.message);
    assert_eq!(job.conflicted_repos, vec!["repo4".to_string()], "both-sides-changed must be recorded as a conflict");
    assert!(conflicting_run.message.contains("CONFLICT"), "outcome message should mention the conflict: {}", conflicting_run.message);

    // Neither side should have been overwritten by the other.
    let src_check = job_root.join("src_check4");
    git(&["clone", src_bare.to_str().unwrap(), src_check.to_str().unwrap()], &job_root);
    git(&["checkout", "main"], &src_check);
    assert_eq!(std::fs::read_to_string(src_check.join("a.txt")).unwrap(), "source-side-change\n");

    let dst_check = job_root.join("dst_check4");
    git(&["clone", dst_bare.to_str().unwrap(), dst_check.to_str().unwrap()], &job_root);
    git(&["checkout", "main"], &dst_check);
    assert_eq!(std::fs::read_to_string(dst_check.join("a.txt")).unwrap(), "dest-side-change\n");

    // A subsequent run must skip the now-conflicted repo entirely rather
    // than silently retrying or force-pushing.
    let followup = run_job(&mut job, &source, &dest);
    assert!(followup.success);
    assert!(
        followup.message.contains("skipped (conflicted, needs manual resolution)"),
        "a conflicted repo must be skipped on subsequent runs, got: {}",
        followup.message
    );
}
