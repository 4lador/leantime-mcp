//! Read-only e2e against a real instance. Opt-in via explicit env vars
//! (LEANTIME_URL + LEANTIME_API_KEY) — no keyring fallback, so plain
//! `cargo test` skips it unless an instance is explicitly targeted
//! (the CI local-e2e job exports both after the docker bootstrap).
//!
//! Loud-skip philosophy (mirrors the TS suite): an empty instance is a
//! legitimate state, but a silent pass on empty data is a vacuous test —
//! it once masked a total data loss. Empty results warn loudly and skip
//! the deep assertions; only the "list projects" check hard-fails on empty
//! (an empty project list means the key is assigned to nothing — a
//! misconfiguration, not a legitimate state).

use serde_json::json;

use leantmcp::client::LeantimeClient;

fn loud_skip(reason: &str) {
    eprintln!(
        "  ⚠ VACUOUS-SKIP: {} (run the exhaustive suite with LEANTIME_E2E=local for full coverage)",
        reason
    );
}

async fn make_client() -> Option<LeantimeClient> {
    let url = std::env::var("LEANTIME_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    let key = std::env::var("LEANTIME_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    Some(LeantimeClient::new(&url, &key))
}

#[tokio::test]
async fn readonly_e2e() {
    let Some(mut client) = make_client().await else {
        eprintln!("  ⚠ SKIPPED: set LEANTIME_URL + LEANTIME_API_KEY to run the readonly e2e");
        return;
    };

    // 1. list projects — empty is a FAILURE: the key is assigned to no project.
    let projects = client
        .call("Projects.getAll", json!({}))
        .await
        .expect("Projects.getAll");
    let projects = projects.as_array().cloned().unwrap_or_default();
    assert!(
        !projects.is_empty(),
        "API key is assigned to no project — fix with `leantmcp key rotate` or assign it in the Leantime UI"
    );
    let pid = projects[0]["id"].clone();
    let pid_str = match &pid {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    println!("✓ list projects: {} project(s)", projects.len());

    // 2. statuses: non-empty map, values have string name + statusType.
    let statuses = client
        .call("tickets.getStatusLabels", json!({"projectId": pid_str}))
        .await
        .expect("getStatusLabels");
    let map = statuses.as_object().expect("statuses should be an object");
    assert!(
        !map.is_empty(),
        "any real project has at least one status label"
    );
    for v in map.values() {
        assert!(
            v.get("name").and_then(|n| n.as_str()).is_some(),
            "status value missing name: {}",
            v
        );
        assert!(
            v.get("statusType").and_then(|t| t.as_str()).is_some(),
            "status value missing statusType: {}",
            v
        );
    }
    println!("✓ statuses: {} label(s)", map.len());

    // 3. tickets with enrichment — iterate projects until one HAS tickets so
    // the deep assertions actually run (empty-first-project would otherwise
    // vacuous-skip). All-empty instance → loud skip, by design.
    let mut tickets: Vec<serde_json::Value> = Vec::new();
    let mut tickets_pid = String::new();
    for p in &projects {
        let p_str = match &p["id"] {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let r = client
            .call(
                "tickets.getAll",
                json!({"searchCriteria": {"currentProject": p_str}, "limit": 100}),
            )
            .await
            .expect("tickets.getAll");
        let list = r.as_array().cloned().unwrap_or_default();
        if !list.is_empty() {
            tickets = list;
            tickets_pid = p_str;
            break;
        }
    }
    if tickets.is_empty() {
        loud_skip("no tickets in ANY project — enrichment not asserted");
    } else {
        let mut enriched = tickets;
        let sm = client
            .get_status_map(&tickets_pid)
            .await
            .expect("status map");
        client.enrich_with_statuses(&mut enriched, &sm);
        let first = &enriched[0];
        assert!(
            first.get("statusLabel").and_then(|v| v.as_str()).is_some(),
            "missing statusLabel: {}",
            first
        );
        assert!(
            first.get("statusType").and_then(|v| v.as_str()).is_some(),
            "missing statusType"
        );
        assert!(
            first.get("statusColor").and_then(|v| v.as_str()).is_some(),
            "missing statusColor"
        );
        println!("✓ tickets: {} enriched", enriched.len());
    }

    // 4. milestones — every item must be type "milestone" — loud skip if none.
    let milestones = client
        .call("tickets.getAll", json!({"searchCriteria": {"currentProject": pid_str, "type": "milestone"}, "limit": 100}))
        .await
        .expect("milestones getAll");
    let milestones = milestones.as_array().cloned().unwrap_or_default();
    if milestones.is_empty() {
        loud_skip("no milestones in the first project");
    } else {
        assert!(
            milestones.iter().all(|m| m["type"] == json!("milestone")),
            "non-milestone in milestone list"
        );
        println!("✓ milestones: {} (all type=milestone)", milestones.len());
    }
}
