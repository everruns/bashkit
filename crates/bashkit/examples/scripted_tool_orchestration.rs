//! Scripted Tool Orchestration Example
//!
//! Demonstrates composing multiple API-like tools into a single OrchestratorTool
//! that an LLM agent can call with bash scripts.
//!
//! Run with: cargo run --example scripted_tool_orchestration
//!
//! This example simulates an e-commerce API with tools for users, orders, and
//! inventory. The OrchestratorTool lets an agent compose these in one call.

use bashkit::{CallableTool, OrchestratorTool, Tool, ToolRequest};

// ============================================================================
// Mock API tools (in real use, these would call actual HTTP endpoints)
// ============================================================================

struct GetUser;

impl CallableTool for GetUser {
    fn name(&self) -> &str {
        "get_user"
    }
    fn description(&self) -> &str {
        "Fetch user by ID. Usage: get_user <id>"
    }
    fn call(&self, args: &[String], _stdin: Option<&str>) -> Result<String, String> {
        let id: u64 = args
            .first()
            .and_then(|s| s.parse().ok())
            .ok_or("usage: get_user <id>")?;

        let users = [
            (1, "Alice", "alice@example.com", "premium"),
            (2, "Bob", "bob@example.com", "basic"),
            (3, "Charlie", "charlie@example.com", "premium"),
        ];

        match users.iter().find(|(uid, ..)| *uid == id) {
            Some((uid, name, email, tier)) => Ok(format!(
                "{{\"id\":{uid},\"name\":\"{name}\",\"email\":\"{email}\",\"tier\":\"{tier}\"}}\n"
            )),
            None => Err(format!("user {} not found", id)),
        }
    }
}

struct ListOrders;

impl CallableTool for ListOrders {
    fn name(&self) -> &str {
        "list_orders"
    }
    fn description(&self) -> &str {
        "List orders for a user. Usage: list_orders <user_id>"
    }
    fn call(&self, args: &[String], _stdin: Option<&str>) -> Result<String, String> {
        let uid: u64 = args
            .first()
            .and_then(|s| s.parse().ok())
            .ok_or("usage: list_orders <user_id>")?;

        let orders = match uid {
            1 => {
                r#"[{"order_id":101,"item":"Laptop","qty":1,"price":999.99},{"order_id":102,"item":"Mouse","qty":2,"price":29.99}]"#
            }
            2 => r#"[{"order_id":201,"item":"Keyboard","qty":1,"price":79.99}]"#,
            3 => r#"[]"#,
            _ => return Err(format!("no orders for user {}", uid)),
        };

        Ok(format!("{orders}\n"))
    }
}

struct GetInventory;

impl CallableTool for GetInventory {
    fn name(&self) -> &str {
        "get_inventory"
    }
    fn description(&self) -> &str {
        "Check inventory for an item. Usage: get_inventory <item_name>"
    }
    fn call(&self, args: &[String], _stdin: Option<&str>) -> Result<String, String> {
        let item = args.first().ok_or("usage: get_inventory <item_name>")?;

        let stock = match item.to_lowercase().as_str() {
            "laptop" => 15,
            "mouse" => 142,
            "keyboard" => 67,
            _ => 0,
        };

        Ok(format!(
            "{{\"item\":\"{}\",\"in_stock\":{}}}\n",
            item, stock
        ))
    }
}

struct CreateDiscount;

impl CallableTool for CreateDiscount {
    fn name(&self) -> &str {
        "create_discount"
    }
    fn description(&self) -> &str {
        "Create a discount code. Usage: create_discount <user_id> <percent>"
    }
    fn call(&self, args: &[String], _stdin: Option<&str>) -> Result<String, String> {
        let uid = args
            .first()
            .ok_or("usage: create_discount <user_id> <percent>")?;
        let pct = args
            .get(1)
            .ok_or("usage: create_discount <user_id> <percent>")?;
        Ok(format!(
            "{{\"code\":\"SAVE{pct}-U{uid}\",\"percent\":{pct},\"user_id\":{uid}}}\n"
        ))
    }
}

// ============================================================================
// Demo: show how OrchestratorTool works
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Scripted Tool Orchestration Demo ===\n");

    // Build the orchestrator with all our API tools
    let mut tool = OrchestratorTool::builder("ecommerce_api")
        .short_description("E-commerce API orchestrator with user, order, and inventory tools")
        .tool(Box::new(GetUser))
        .tool(Box::new(ListOrders))
        .tool(Box::new(GetInventory))
        .tool(Box::new(CreateDiscount))
        .env("STORE_NAME", "Bashkit Shop")
        .build();

    // ---- Show what the LLM sees ----
    println!("--- Tool name ---");
    println!("{}\n", tool.name());

    println!("--- System prompt (what goes in LLM system message) ---");
    println!("{}", tool.system_prompt());

    // ---- Demo 1: Simple single tool call ----
    println!("--- Demo 1: Single tool call ---");
    let resp = tool
        .execute(ToolRequest {
            commands: "get_user 1".to_string(),
        })
        .await;
    println!("$ get_user 1");
    println!("{}", resp.stdout);

    // ---- Demo 2: Pipeline with jq ----
    println!("--- Demo 2: Pipeline with jq ---");
    let resp = tool
        .execute(ToolRequest {
            commands: "get_user 1 | jq -r '.name'".to_string(),
        })
        .await;
    println!("$ get_user 1 | jq -r '.name'");
    println!("{}", resp.stdout);

    // ---- Demo 3: Multi-step orchestration ----
    println!("--- Demo 3: Multi-step orchestration ---");
    let script = r#"
        user=$(get_user 1)
        name=$(echo "$user" | jq -r '.name')
        tier=$(echo "$user" | jq -r '.tier')
        orders=$(list_orders 1)
        total=$(echo "$orders" | jq '[.[].price] | add')
        count=$(echo "$orders" | jq 'length')
        echo "Customer: $name (tier: $tier)"
        echo "Orders: $count, Estimated total: $total"
    "#;
    let resp = tool
        .execute(ToolRequest {
            commands: script.to_string(),
        })
        .await;
    println!("$ <multi-step script>");
    print!("{}", resp.stdout);
    println!();

    // ---- Demo 4: Loop + conditional ----
    println!("--- Demo 4: Loop with conditional ---");
    let script = r#"
        for uid in 1 2 3; do
            user=$(get_user $uid)
            name=$(echo "$user" | jq -r '.name')
            tier=$(echo "$user" | jq -r '.tier')
            if [ "$tier" = "premium" ]; then
                echo "$name is premium - creating discount"
                create_discount $uid 20 | jq -r '.code'
            else
                echo "$name is $tier - no discount"
            fi
        done
    "#;
    let resp = tool
        .execute(ToolRequest {
            commands: script.to_string(),
        })
        .await;
    println!("$ <loop with conditional>");
    print!("{}", resp.stdout);
    println!();

    // ---- Demo 5: Inventory check with error handling ----
    println!("--- Demo 5: Error handling ---");
    let script = r#"
        for item in Laptop Mouse Keyboard Widget; do
            result=$(get_inventory "$item")
            stock=$(echo "$result" | jq '.in_stock')
            if [ "$stock" -eq 0 ]; then
                echo "$item: OUT OF STOCK"
            else
                echo "$item: $stock in stock"
            fi
        done
    "#;
    let resp = tool
        .execute(ToolRequest {
            commands: script.to_string(),
        })
        .await;
    println!("$ <inventory check>");
    print!("{}", resp.stdout);
    println!();

    // ---- Demo 6: Data aggregation ----
    println!("--- Demo 6: Aggregate data across tools ---");
    let script = r#"
        echo "=== $STORE_NAME Report ==="
        for uid in 1 2; do
            name=$(get_user $uid | jq -r '.name')
            orders=$(list_orders $uid)
            count=$(echo "$orders" | jq 'length')
            echo "$name: $count orders"
        done
    "#;
    let resp = tool
        .execute(ToolRequest {
            commands: script.to_string(),
        })
        .await;
    println!("$ <aggregate report>");
    print!("{}", resp.stdout);

    println!("\n=== Demo Complete ===");
    Ok(())
}
