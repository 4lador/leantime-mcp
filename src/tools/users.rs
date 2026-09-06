//! `leantime_list_users` — simplified user list for assignment.

use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use super::shared::{get_users_simplified, tool};
use super::{error_result, ok_result, ClientRef, Tool};

fn h_list_users(_a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        match get_users_simplified(&mut c).await {
            Ok(users) => {
                let l: Vec<Value> = users
                    .iter()
                    .map(|(id, name)| json!({"id": id, "name": name}))
                    .collect();
                ok_result(&json!(l))
            }
            Err(e) => error_result(&e),
        }
    })
}

pub(super) fn tools() -> Vec<Tool> {
    vec![tool(
        "leantime_list_users",
        "List all users (id, name) — for assignment",
        vec![],
        vec![],
        Box::new(h_list_users),
    )]
}
