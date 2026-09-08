//! Execution-profile tests in a dedicated process: LEANTIME_MCP_PROFILE
//! is process-global and would leak into every parallel tools_test lookup.
//! The two cases run inside ONE test for the same reason — they share the
//! process environment and must not race each other.

use leantmcp::tools;

#[test]
fn readonly_profile_filters_and_default_restores() {
    // Default: full registry
    std::env::remove_var("LEANTIME_MCP_PROFILE");
    let full = tools::create_registry();
    assert_eq!(full.len(), 42, "default profile keeps all tools");

    // Readonly: write and destructive handlers are removed at construction
    std::env::set_var("LEANTIME_MCP_PROFILE", "readonly");
    let readonly = tools::create_registry();
    std::env::remove_var("LEANTIME_MCP_PROFILE");

    assert!(
        !readonly.iter().any(|t| t.name == "leantime_create_ticket"),
        "create tool must not exist in readonly"
    );
    assert!(
        !readonly.iter().any(|t| t.name == "leantime_delete_ticket"),
        "delete tool must not exist in readonly"
    );
    assert!(
        !readonly
            .iter()
            .any(|t| t.name == "leantime_bulk_update_tickets"),
        "bulk write tool must not exist in readonly"
    );
    // reads stay: list, get, project_context, backup (API-read-only)
    assert!(readonly.iter().any(|t| t.name == "leantime_list_tickets"));
    assert!(readonly
        .iter()
        .any(|t| t.name == "leantime_project_context"));
    assert!(readonly.iter().any(|t| t.name == "leantime_backup_project"));
    // every remaining tool is annotated read-only
    assert!(readonly.iter().all(|t| t.annotations.read_only));

    // And back: removing the variable restores the full registry
    let again = tools::create_registry();
    assert_eq!(again.len(), 42);
}
