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
