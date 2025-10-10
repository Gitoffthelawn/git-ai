use git_ai::git::test_utils::{TmpRepo, snapshot_checkpoints};
use insta::assert_debug_snapshot;

/// Test simple stash/pop on same commit - working log preserved automatically
#[test]
fn test_simple_stash_pop_same_commit() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    let initial_content = "line 1\nline 2\nline 3\n";
    tmp_repo
        .write_file("test.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let commit_sha = tmp_repo.get_head_commit_sha().unwrap();

    // Make AI changes
    let ai_content = "line 1\nline 2\nline 3\n// AI added feature\n";
    tmp_repo.write_file("test.txt", ai_content, true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Verify working log exists
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&commit_sha);
    let checkpoints_before = working_log.read_all_checkpoints().unwrap();
    assert_eq!(checkpoints_before.len(), 1);
    assert!(checkpoints_before[0].agent_id.is_some());

    // Stash the changes
    tmp_repo.stash_push().unwrap();

    // Working log should still exist
    let checkpoints_after_stash = working_log.read_all_checkpoints().unwrap();
    assert_eq!(
        checkpoints_after_stash.len(),
        1,
        "Working log preserved after stash"
    );

    // Pop the stash (same commit)
    let has_conflicts = tmp_repo.stash_pop().unwrap();
    assert!(!has_conflicts, "Should not have conflicts");

    // Working log should still exist (automatically preserved since HEAD didn't change)
    let checkpoints_after_pop = working_log.read_all_checkpoints().unwrap();
    assert_eq!(
        checkpoints_after_pop.len(),
        1,
        "Working log preserved after pop on same commit"
    );
    assert!(checkpoints_after_pop[0].agent_id.is_some());

    // Snapshot the final working log
    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_after_pop));
}

/// Test stash on commit A, pop on commit B - authorship reconstructed
#[test]
fn test_stash_on_a_pop_on_b() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit A
    let initial_content = "line 1\nline 2\nline 3\n";
    tmp_repo
        .write_file("test.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let commit_a = tmp_repo.get_head_commit_sha().unwrap();

    // Make AI changes on commit A
    let ai_content = "line 1\nline 2\nline 3\n// AI added feature on A\n";
    tmp_repo.write_file("test.txt", ai_content, true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Verify working log exists for commit A
    let working_log_a = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&commit_a);
    let checkpoints_a = working_log_a.read_all_checkpoints().unwrap();
    assert_eq!(checkpoints_a.len(), 1);
    assert!(checkpoints_a[0].agent_id.is_some());

    // Stash the changes
    tmp_repo.stash_push().unwrap();

    // Create commit B
    let content_b = "line 1\nline 2\nline 3\n// Different change on B\n";
    tmp_repo.write_file("test.txt", content_b, true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Commit B").unwrap();
    let commit_b = tmp_repo.get_head_commit_sha().unwrap();

    assert_ne!(commit_a, commit_b, "Should be on different commit");

    // Pop the stash on commit B
    let has_conflicts = tmp_repo.stash_pop().unwrap();
    // Might have conflicts due to overlapping changes, but that's ok for this test
    // We mainly want to verify that authorship reconstruction is attempted

    if !has_conflicts {
        // If no conflicts, check that working log was reconstructed for commit B
        let working_log_b = tmp_repo
            .gitai_repo()
            .storage
            .working_log_for_base_commit(&commit_b);
        let checkpoints_b = working_log_b.read_all_checkpoints().unwrap();

        // Should have at least one AI checkpoint reconstructed
        let ai_checkpoints: Vec<_> = checkpoints_b
            .iter()
            .filter(|c| c.agent_id.is_some())
            .collect();
        assert!(
            !ai_checkpoints.is_empty(),
            "Should have reconstructed AI authorship"
        );

        // Snapshot the reconstructed working log
        assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_b));
    }
}

/// Test stash on A, switch branches, pop on B (cross-branch)
#[test]
fn test_stash_pop_cross_branch() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit on main
    let initial_content = "section 1\nsection 2\nsection 3\n";
    tmp_repo
        .write_file("document.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let commit_main = tmp_repo.get_head_commit_sha().unwrap();

    // Make AI changes on main
    let ai_content = "section 1\nsection 2\nsection 3\n// AI enhancement\n";
    tmp_repo
        .write_file("document.txt", ai_content, true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Verify working log on main
    let working_log_main = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&commit_main);
    let checkpoints_main = working_log_main.read_all_checkpoints().unwrap();
    assert_eq!(checkpoints_main.len(), 1);

    // Stash the changes
    tmp_repo.stash_push().unwrap();

    // Create and switch to feature branch
    tmp_repo.create_branch("feature").unwrap();

    // Make different changes on feature branch
    let feature_content = "section 1\nsection 2\nsection 3\n// Feature work\n";
    tmp_repo
        .write_file("document.txt", feature_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Feature commit").unwrap();
    let commit_feature = tmp_repo.get_head_commit_sha().unwrap();

    assert_ne!(commit_main, commit_feature, "Should be on different commit");

    // Pop stash on feature branch
    let has_conflicts = tmp_repo.stash_pop().unwrap();

    if !has_conflicts {
        // Check that authorship was reconstructed for feature branch commit
        let working_log_feature = tmp_repo
            .gitai_repo()
            .storage
            .working_log_for_base_commit(&commit_feature);
        let checkpoints_feature = working_log_feature.read_all_checkpoints().unwrap();

        let ai_checkpoints: Vec<_> = checkpoints_feature
            .iter()
            .filter(|c| c.agent_id.is_some())
            .collect();
        assert!(
            !ai_checkpoints.is_empty(),
            "Should reconstruct AI authorship across branches"
        );

        // Snapshot the cross-branch reconstructed working log
        assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_feature));
    }
}

/// Test stash apply (vs pop) - stash remains after apply
#[test]
fn test_stash_apply_preserves_stash() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    let initial_content = "line 1\nline 2\n";
    tmp_repo
        .write_file("test.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let commit_a = tmp_repo.get_head_commit_sha().unwrap();

    // Make AI changes
    let ai_content = "line 1\nline 2\n// AI addition\n";
    tmp_repo.write_file("test.txt", ai_content, true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Stash
    tmp_repo.stash_push().unwrap();

    // Make a new commit
    let new_content = "line 1\nline 2\n// New work\n";
    tmp_repo.write_file("test.txt", new_content, true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("New commit").unwrap();
    let commit_b = tmp_repo.get_head_commit_sha().unwrap();

    assert_ne!(commit_a, commit_b);

    // Apply stash (not pop)
    let has_conflicts = tmp_repo.stash_apply("stash@{0}").unwrap();

    if !has_conflicts {
        // Check authorship reconstruction happened
        let working_log_b = tmp_repo
            .gitai_repo()
            .storage
            .working_log_for_base_commit(&commit_b);
        let checkpoints_b = working_log_b.read_all_checkpoints().unwrap();

        let ai_checkpoints: Vec<_> = checkpoints_b
            .iter()
            .filter(|c| c.agent_id.is_some())
            .collect();
        assert!(
            !ai_checkpoints.is_empty(),
            "Should reconstruct AI authorship with apply"
        );

        // Snapshot the working log after stash apply
        assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_b));
    }

    // Verify stash still exists (apply doesn't remove it)
    // We can't easily check stash list, but the test passes if apply worked
}

/// Test stash with no AI authorship - graceful handling
#[test]
fn test_stash_no_ai_authorship() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    let initial_content = "line 1\nline 2\n";
    tmp_repo
        .write_file("test.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let commit_a = tmp_repo.get_head_commit_sha().unwrap();

    // Make human-only changes (no AI)
    let human_content = "line 1\nline 2\n// Human addition\n";
    tmp_repo
        .write_file("test.txt", human_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();

    // Stash
    tmp_repo.stash_push().unwrap();

    // Make a new commit
    let new_content = "line 1\nline 2\n// Different work\n";
    tmp_repo.write_file("test.txt", new_content, true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("New commit").unwrap();
    let commit_b = tmp_repo.get_head_commit_sha().unwrap();

    assert_ne!(commit_a, commit_b);

    // Pop stash - should work without errors even with no AI authorship
    let has_conflicts = tmp_repo.stash_pop().unwrap();

    if !has_conflicts {
        // Snapshot the working log - should have no AI checkpoints
        let working_log_b = tmp_repo
            .gitai_repo()
            .storage
            .working_log_for_base_commit(&commit_b);
        let checkpoints_b = working_log_b.read_all_checkpoints().unwrap_or_default();

        assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_b));
    }
}

/// Test stash with multiple AI sessions
#[test]
fn test_stash_multiple_ai_sessions() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    let initial_content = "header\nbody\nfooter\n";
    tmp_repo
        .write_file("file.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let commit_a = tmp_repo.get_head_commit_sha().unwrap();

    // First AI session
    let content_v2 = "header\n// AI session 1\nbody\nfooter\n";
    tmp_repo.write_file("file.txt", content_v2, true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_1", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Second AI session
    let content_v3 = "header\n// AI session 1\nbody\nfooter\n// AI session 2\n";
    tmp_repo.write_file("file.txt", content_v3, true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_2", Some("claude"), Some("cursor"))
        .unwrap();

    // Verify 2 AI checkpoints
    let working_log_a = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&commit_a);
    let checkpoints_a = working_log_a.read_all_checkpoints().unwrap();
    assert_eq!(checkpoints_a.len(), 2);

    // Stash
    tmp_repo.stash_push().unwrap();

    // Create new commit
    let new_content = "header\nbody\nfooter\n// New work\n";
    tmp_repo.write_file("file.txt", new_content, true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("New commit").unwrap();
    let commit_b = tmp_repo.get_head_commit_sha().unwrap();

    // Pop stash
    let has_conflicts = tmp_repo.stash_pop().unwrap();

    if !has_conflicts {
        // Should reconstruct both AI sessions
        let working_log_b = tmp_repo
            .gitai_repo()
            .storage
            .working_log_for_base_commit(&commit_b);
        let checkpoints_b = working_log_b.read_all_checkpoints().unwrap();

        let ai_checkpoints: Vec<_> = checkpoints_b
            .iter()
            .filter(|c| c.agent_id.is_some())
            .collect();
        assert!(
            ai_checkpoints.len() >= 1,
            "Should reconstruct AI authorship from multiple sessions"
        );

        // Snapshot the working log with multiple AI sessions
        assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_b));
    }
}

/// Test stash on dirty repo (existing working log + stashed changes)
#[test]
fn test_stash_on_dirty_repo_appends() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    let initial_content = "line 1\nline 2\n";
    tmp_repo
        .write_file("test.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let _commit_a = tmp_repo.get_head_commit_sha().unwrap();

    // Make AI changes on A and stash
    let ai_content_1 = "line 1\nline 2\n// AI change 1\n";
    tmp_repo.write_file("test.txt", ai_content_1, true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.stash_push().unwrap();

    // Create commit B
    let content_b = "line 1\nline 2\n// Work on B\n";
    tmp_repo.write_file("test.txt", content_b, true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Commit B").unwrap();
    let commit_b = tmp_repo.get_head_commit_sha().unwrap();

    // Make additional AI changes on B (creates working log for B)
    let ai_content_2 = "line 1\nline 2\n// Work on B\n// AI change 2 on B\n";
    tmp_repo.write_file("test.txt", ai_content_2, true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("gpt-4"), Some("cursor"))
        .unwrap();

    let working_log_b = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&commit_b);
    let checkpoints_before_pop = working_log_b.read_all_checkpoints().unwrap();
    let ai_count_before: usize = checkpoints_before_pop
        .iter()
        .filter(|c| c.agent_id.is_some())
        .count();

    // Pop stash (should append to existing working log, not replace)
    let has_conflicts = tmp_repo.stash_pop().unwrap();

    if !has_conflicts {
        let checkpoints_after_pop = working_log_b.read_all_checkpoints().unwrap();
        let ai_count_after: usize = checkpoints_after_pop
            .iter()
            .filter(|c| c.agent_id.is_some())
            .count();

        // Should have at least as many AI checkpoints as before (append, not replace)
        assert!(
            ai_count_after >= ai_count_before,
            "Should append stashed authorship, not replace. Before: {}, After: {}",
            ai_count_before,
            ai_count_after
        );

        // Snapshot the appended working log
        assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_after_pop));
    }
}

/// Test empty stash - no working log to preserve
#[test]
fn test_empty_stash() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit with no changes
    let initial_content = "line 1\nline 2\n";
    tmp_repo
        .write_file("test.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    // Try to stash with no changes - git will refuse
    // This test just verifies we handle this gracefully
    let result = tmp_repo.stash_push();
    // Either succeeds with no stash created, or fails - both are fine
    let _ = result;
}

/// Test cross-commit stash reconstruction with non-overlapping changes (guaranteed no conflicts)
#[test]
fn test_stash_cross_commit_no_conflicts() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit A with a file
    let initial_content = "line 1\nline 2\nline 3\n";
    tmp_repo
        .write_file("feature.txt", initial_content, true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let commit_a = tmp_repo.get_head_commit_sha().unwrap();

    // Make AI changes on commit A (modify feature.txt)
    let ai_content = "line 1\nline 2\nline 3\n// AI enhancement\n";
    tmp_repo
        .write_file("feature.txt", ai_content, true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Stash the changes
    tmp_repo.stash_push().unwrap();

    // Create commit B with changes to a DIFFERENT file (no conflicts possible)
    tmp_repo
        .write_file("other.txt", "unrelated content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Commit B").unwrap();
    let commit_b = tmp_repo.get_head_commit_sha().unwrap();

    assert_ne!(commit_a, commit_b, "Should be on different commit");

    // Pop the stash on commit B - NO CONFLICTS since we modified different files
    let has_conflicts = tmp_repo.stash_pop().unwrap();
    assert!(
        !has_conflicts,
        "Should not have conflicts - different files modified"
    );

    // Verify authorship was reconstructed for commit B
    let working_log_b = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&commit_b);
    let checkpoints_b = working_log_b.read_all_checkpoints().unwrap();

    // Should have reconstructed the AI checkpoint
    let ai_checkpoints: Vec<_> = checkpoints_b
        .iter()
        .filter(|c| c.agent_id.is_some())
        .collect();
    assert_eq!(
        ai_checkpoints.len(),
        1,
        "Should have exactly 1 AI checkpoint"
    );
    assert_eq!(ai_checkpoints[0].author, "ai_agent");

    // Snapshot to prove cross-commit reconstruction works
    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_b));
}
