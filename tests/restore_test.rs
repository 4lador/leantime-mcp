//! Unit tests for the restore module: topological sort, milestone dedup,
//! status extraction/mapping.

use leantmcp::restore::*;
use serde_json::json;

fn ticket(id: &str, dep: &str, ttype: &str) -> serde_json::Value {
    json!({
        "id": id,
        "headline": format!("ticket {}", id),
        "type": ttype,
        "dependingTicketId": if dep.is_empty() { json!(0) } else { json!(dep) },
    })
}

// ---------------------------------------------------------------- topological sort

#[test]
fn topo_sort_parents_before_children() {
    let tickets = vec![
        ticket("1", "", "task"),  // parent
        ticket("2", "1", "task"), // child of 1
        ticket("3", "2", "task"), // grandchild of 2
        ticket("4", "", "task"),  // unrelated parent
    ];
    let (sorted, warnings) = topological_sort(&tickets);
    assert!(warnings.is_empty());
    assert_eq!(sorted.len(), 4);

    // Verify ordering: 1 before 2, 2 before 3
    let pos: Vec<&str> = sorted.iter().map(|t| t["id"].as_str().unwrap()).collect();
    let p1 = pos.iter().position(|&x| x == "1").unwrap();
    let p2 = pos.iter().position(|&x| x == "2").unwrap();
    let p3 = pos.iter().position(|&x| x == "3").unwrap();
    assert!(p1 < p2, "parent 1 must come before child 2");
    assert!(p2 < p3, "parent 2 must come before grandchild 3");
}

#[test]
fn topo_sort_orphan_becomes_top_level() {
    let tickets = vec![
        ticket("1", "", "task"),
        ticket("2", "999", "task"), // orphan — parent 999 not in backup
    ];
    let (sorted, warnings) = topological_sort(&tickets);
    assert_eq!(sorted.len(), 2);
    assert!(!warnings.is_empty(), "should warn about orphan");
    assert!(warnings[0].contains("orphan"), "warning: {}", warnings[0]);
    assert!(
        warnings[0].contains("999"),
        "warning should mention parent: {}",
        warnings[0]
    );
}

#[test]
fn topo_sort_circular_dependency() {
    let tickets = vec![
        ticket("1", "3", "task"), // 1 depends on 3
        ticket("2", "1", "task"), // 2 depends on 1
        ticket("3", "2", "task"), // 3 depends on 2 → cycle
    ];
    let (sorted, warnings) = topological_sort(&tickets);
    assert_eq!(sorted.len(), 3, "all tickets should still be created");
    assert!(
        !warnings.is_empty(),
        "should warn about circular dependency"
    );
}

#[test]
fn topo_sort_no_dependencies() {
    let tickets = vec![
        ticket("a", "", "task"),
        ticket("b", "", "task"),
        ticket("c", "", "task"),
    ];
    let (sorted, warnings) = topological_sort(&tickets);
    assert_eq!(sorted.len(), 3);
    assert!(warnings.is_empty());
}

// ---------------------------------------------------------------- status extraction

#[test]
fn extract_statuses_counts_unique() {
    let backup = json!({
        "tickets": [
            {"id": 1, "status": 0, "statusLabel": "New", "statusType": "NEW"},
            {"id": 2, "status": 0, "statusLabel": "New", "statusType": "NEW"},
            {"id": 3, "status": 3, "statusLabel": "Done", "statusType": "DONE"},
            {"id": 4, "status": 0, "statusLabel": "New", "statusType": "NEW"},
            {"id": 5, "status": 5, "statusLabel": "Blocked", "statusType": "BLOCKED"},
        ]
    });
    let statuses = extract_backup_statuses(&backup);
    assert_eq!(statuses.len(), 3, "should have 3 unique statuses");

    // Find each status
    let new = statuses.iter().find(|(_, l, _, _)| l == "New").unwrap();
    assert_eq!(new.3, 3, "3 tickets with status New");

    let done = statuses.iter().find(|(_, l, _, _)| l == "Done").unwrap();
    assert_eq!(done.3, 1);

    let blocked = statuses.iter().find(|(_, l, _, _)| l == "Blocked").unwrap();
    assert_eq!(blocked.3, 1);
}

#[test]
fn detect_gaps_finds_missing_labels() {
    let backup_statuses = vec![
        (0, "New".to_string(), "NEW".to_string(), 10),
        (3, "Done".to_string(), "DONE".to_string(), 5),
        (5, "Blocked".to_string(), "BLOCKED".to_string(), 3),
    ];
    let project_statuses = json!({
        "0": {"name": "New", "statusType": "NEW"},
        "1": {"name": "In Progress", "statusType": "IN_PROGRESS"},
        "3": {"name": "Done", "statusType": "DONE"},
    });

    let gaps = detect_status_gaps(&backup_statuses, &project_statuses);
    assert_eq!(gaps.len(), 1, "only 'Blocked' should be a gap");
    assert_eq!(gaps[0].1, "Blocked");
    assert_eq!(gaps[0].3, 3, "3 tickets use Blocked");
}

#[test]
fn detect_gaps_case_insensitive() {
    let backup_statuses = vec![(0, "NEW".to_string(), "NEW".to_string(), 5)];
    let project_statuses = json!({
        "0": {"name": "new", "statusType": "NEW"},  // lowercase
    });

    let gaps = detect_status_gaps(&backup_statuses, &project_statuses);
    assert!(gaps.is_empty(), "NEW should match new (case-insensitive)");
}

#[test]
fn build_mapping_by_label() {
    let backup_statuses = vec![
        (0, "New".to_string(), "NEW".to_string(), 10),
        (5, "Blocked".to_string(), "BLOCKED".to_string(), 3),
    ];
    let project_statuses = json!({
        "0": {"name": "New", "statusType": "NEW"},
        "7": {"name": "Blocked", "statusType": "BLOCKED"},
    });

    let mapping = build_status_mapping(&backup_statuses, &project_statuses);
    assert_eq!(mapping.get("New"), Some(&0));
    assert_eq!(mapping.get("Blocked"), Some(&7), "should map to ID 7");
}

#[test]
fn build_mapping_no_match_returns_none() {
    let backup_statuses = vec![(5, "Blocked".to_string(), "BLOCKED".to_string(), 3)];
    let project_statuses = json!({
        "0": {"name": "New"},
        "1": {"name": "In Progress"},
    });

    let mapping = build_status_mapping(&backup_statuses, &project_statuses);
    assert!(
        mapping.get("Blocked").is_none(),
        "should not map if label not found"
    );
}

// ---------------------------------------------------------------- status label normalization

#[test]
fn extract_statuses_normalizes_placeholder_labels() {
    let backup = json!({
        "tickets": [
            {"id": 1, "status": 0, "statusLabel": "?", "statusType": "DONE"},
            {"id": 2, "status": 0, "statusLabel": "", "statusType": "DONE"},
            {"id": 3, "status": 0, "statusLabel": null, "statusType": "DONE"},
            {"id": 4, "status": 3, "statusLabel": "A Faire", "statusType": "NEW"},
        ]
    });
    let statuses = extract_backup_statuses(&backup);

    // The 3 tickets with status 0 should have label "DONE" (from statusType fallback)
    let done = statuses.iter().find(|(_, l, _, c)| l == "DONE" && *c == 3);
    assert!(
        done.is_some(),
        "placeholder labels should be normalized to statusType, got: {:?}",
        statuses
    );

    // The ticket with "A Faire" should keep its actual label
    let faire = statuses
        .iter()
        .find(|(_, l, _, c)| l == "A Faire" && *c == 1);
    assert!(faire.is_some(), "real labels should be preserved");
}

#[test]
fn build_mapping_status_type_fallback() {
    // Backup has French labels, project has English — different labels but same types
    let backup_statuses = vec![
        (3, "A Faire".to_string(), "NEW".to_string(), 10),
        (4, "En cours".to_string(), "INPROGRESS".to_string(), 5),
        (0, "DONE".to_string(), "DONE".to_string(), 20),
    ];
    let project_statuses = json!({
        "3": {"name": "New", "statusType": "NEW"},
        "4": {"name": "In Progress", "statusType": "INPROGRESS"},
        "0": {"name": "Done", "statusType": "DONE"},
    });

    let mapping = build_status_mapping(&backup_statuses, &project_statuses);

    // Label "A Faire" doesn't match "New" but both have type NEW
    assert_eq!(
        mapping.get("A Faire"),
        Some(&3),
        "should fallback to statusType NEW match"
    );
    assert_eq!(
        mapping.get("En cours"),
        Some(&4),
        "should fallback to statusType INPROGRESS match"
    );
    assert_eq!(
        mapping.get("DONE"),
        Some(&0),
        "should fallback to statusType DONE match"
    );
}

#[test]
fn build_mapping_label_match_takes_priority_over_type() {
    // If label matches AND type matches, use the label match
    let backup_statuses = vec![(0, "Done".to_string(), "DONE".to_string(), 5)];
    let project_statuses = json!({
        "0": {"name": "Different Name", "statusType": "DONE"},
        "5": {"name": "Done", "statusType": "DONE"},
    });

    let mapping = build_status_mapping(
        &backup_statuses,
        &type_fallback_project(&backup_statuses, &project_statuses),
    );
    // "Done" matches status 5 by label, not status 0 by type
    assert_eq!(mapping.get("Done"), Some(&5));
}

fn type_fallback_project(
    _backup: &[BackupStatus],
    project: &serde_json::Value,
) -> serde_json::Value {
    project.clone()
}

#[test]
fn build_mapping_no_type_match_still_returns_none() {
    // Backup has a type that doesn't exist in the project at all
    let backup_statuses = vec![(7, "Blocked".to_string(), "BLOCKED".to_string(), 3)];
    let project_statuses = json!({
        "0": {"name": "New", "statusType": "NEW"},
        "1": {"name": "In Progress", "statusType": "INPROGRESS"},
        "2": {"name": "Done", "statusType": "DONE"},
    });

    let mapping = build_status_mapping(&backup_statuses, &project_statuses);
    assert!(
        mapping.get("Blocked").is_none(),
        "no match → gap → interactive resolution needed"
    );
}
