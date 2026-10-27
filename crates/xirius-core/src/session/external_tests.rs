use super::*;

#[test]
fn external_acceptance_checks_store_and_saved_fingerprint_before_mutation_without_io() {
    let root = tempfile::tempdir().unwrap();
    let mut current = Session::new("Fixture");
    current.save_as(root.path().join("song")).unwrap();
    let mut other = Session::new("Elsewhere");
    other.save_as(root.path().join("other")).unwrap();
    let wrong =
        ExternalProject::read(other.store().unwrap().clone(), current.disk_hash.clone()).unwrap();
    let old = current.project_arc();
    current.write_in_background(true);
    {
        let _ui = crate::io_test::forbid_ui_io();
        assert!(current.adopt_disk_project(wrong, &Author::Human).is_err());
        assert!(Arc::ptr_eq(&old, &current.project_arc()));
    }
    let stale =
        ExternalProject::read(current.store().unwrap().clone(), current.disk_hash.clone()).unwrap();
    current
        .run_value(
            "track.add.audio",
            json!({"name":"Local"}),
            &RunOptions::human(),
        )
        .unwrap();
    current.save().unwrap();
    let saved = current.project_arc();
    {
        let _ui = crate::io_test::forbid_ui_io();
        assert!(current.adopt_disk_project(stale, &Author::Human).is_err());
        assert!(Arc::ptr_eq(&saved, &current.project_arc()));
    }
}

#[test]
fn accepted_external_project_queues_step_then_manifest_and_remains_one_undo() {
    let root = tempfile::tempdir().unwrap();
    let mut current = Session::new("Fixture");
    current.save_as(root.path()).unwrap();
    let mut external = Session::open(root.path()).unwrap();
    external
        .run_value(
            "track.add.audio",
            json!({"name":"External"}),
            &RunOptions::human(),
        )
        .unwrap();
    external.save().unwrap();
    let prepared =
        ExternalProject::read(current.store().unwrap().clone(), current.disk_hash.clone()).unwrap();
    let before = current.cursor();
    current.write_in_background(true);
    {
        let _ui = crate::io_test::forbid_ui_io();
        assert!(
            current
                .adopt_disk_project(prepared, &Author::Human)
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(current.cursor(), before + 1);
    assert!(!current.is_dirty());
    current.flush_writes();
    assert!(current.take_write_failures().is_empty());
    current.undo(1).unwrap();
    assert!(
        !current
            .project()
            .tracks
            .values()
            .any(|t| t.name == "External")
    );
}
