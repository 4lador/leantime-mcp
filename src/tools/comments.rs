use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use crate::markdown::markdown_to_html;

use super::shared::*;
use super::{error_result, ok_result, ClientRef, Tool, ToolAnnotations};

fn h_list_comments(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let tid = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        match c
            .call(
                "comments.getComments",
                json!({"module": "ticket", "entityId": tid}),
            )
            .await
        {
            Ok(r) => ok_result(&json!(r.as_array().cloned().unwrap_or_default())),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_add_comment(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let tid = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        let text = a.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let father = a.get("father").and_then(|v| v.as_i64()).unwrap_or(0);
        let html = markdown_to_html(text);
        // addComment requires the full `entity` object — rebuild it from the ticket.
        let ticket = match c.call("tickets.getTicket", json!({"id": tid})).await {
            Ok(t) => t,
            Err(e) => return error_result(&e.to_string()),
        };
        if ticket.is_boolean() || is_leantime_error(&ticket) {
            return error_result(&format!("Ticket {} not found.", tid));
        }
        let add = c.call("comments.addComment", json!({
            "values": {"text": html, "father": father}, "module": "ticket", "entityId": tid,
            "entity": {"id": tid, "type": ticket.get("type").cloned().unwrap_or(json!("task")), "headline": ticket.get("headline")}
        })).await;
        if add.is_err() {
            // Leantime v3.7.3 bug: the comment row is inserted, then the
            // notification build crashes on entity property access via JSON-RPC.
            // Verify the comment actually landed before surfacing an error.
            let landed = c
                .call(
                    "comments.getComments",
                    json!({"module": "ticket", "entityId": tid}),
                )
                .await
                .ok()
                .and_then(|comments| comments.as_array().cloned())
                .map(|arr| {
                    arr.iter()
                        .any(|cm| cm.get("text").and_then(|t| t.as_str()) == Some(html.as_str()))
                })
                .unwrap_or(false);
            if !landed {
                return error_result(&add.err().unwrap().to_string());
            }
        }
        ok_result(&json!({ "ok": true, "ticketId": tid }))
    })
}

fn h_update_comment(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let id = a.get("commentId").and_then(|v| v.as_str()).unwrap_or("");
        let text = a.get("text").and_then(|v| v.as_str()).unwrap_or("");
        match c
            .call(
                "comments.editComment",
                json!({"values": {"text": markdown_to_html(text)}, "id": id}),
            )
            .await
        {
            Ok(r) => ok_result(&json!({ "ok": r == json!(true), "commentId": id })),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_delete_comment(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        if let Err(e) = check_destructive(a.get("confirm").and_then(|v| v.as_bool()), "comment") {
            return error_result(&e);
        }
        let mut c = cl.lock().await;
        let id = a.get("commentId").and_then(|v| v.as_str()).unwrap_or("");
        match c
            .call("comments.deleteComment", json!({ "commentId": id }))
            .await
        {
            Ok(r) => ok_result(&json!({ "deleted": r == json!(true), "commentId": id })),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

pub(super) fn tools() -> Vec<Tool> {
    let md = MARKDOWN_HINT;
    vec![
        tool("leantime_list_comments", "List the discussion comments of a ticket",
            vec![rs("ticketId", "The ticket ID")], vec!["ticketId"], Box::new(h_list_comments)),
        tool_with_annotations("leantime_add_comment", format!("Add a comment to a ticket's discussion. The text is {}.", md),
            vec![rs("ticketId", "The ticket ID"), rs("text", format!("Comment body in {}", md)), ("father".to_string(), json!({"type": "number", "description": "Parent comment ID for a reply (omit or 0 for a top-level comment)", "optional": true}))],
            vec!["ticketId", "text"], Box::new(h_add_comment), ToolAnnotations::write()),
        tool_with_annotations("leantime_update_comment", format!("Edit an existing comment. The text is {}.", md),
            vec![rs("commentId", "The comment ID"), rs("text", format!("New comment body in {}", md))],
            vec!["commentId", "text"], Box::new(h_update_comment), ToolAnnotations::write()),
        tool_with_annotations("leantime_delete_comment", "Delete a comment. Destructive: requires explicit user approval (confirm: true) unless LEANTIME_MCP_DESTRUCTIVE_POLICY is set otherwise.",
            vec![rs("commentId", "The comment ID"), ob("confirm", "MUST be true to actually delete (ask the user for explicit approval first)")],
            vec!["commentId"], Box::new(h_delete_comment), ToolAnnotations::destructive()),
    ]
}
