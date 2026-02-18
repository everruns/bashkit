//! Scripted Tool Example
//!
//! Demonstrates composing multiple API-like tools (ToolDef + closures) into a
//! single ScriptedTool that an LLM agent can call with bash scripts.
//!
//! Run with: cargo run --example scripted_tool --features scripted_tool
//!
//! This example simulates an e-commerce API with tools for users, orders, and
//! inventory. The ScriptedTool lets an agent compose these in one call.

use bashkit::{ScriptedTool, Tool, ToolRequest};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Scripted Tool Demo ===\n");

    // Build the orchestrator with tool definitions + callbacks.
    // In production the callbacks would call real APIs.
    let mut tool = ScriptedTool::builder("ecommerce_api")
        .short_description("E-commerce API orchestrator with user, order, and inventory tools")
        .tool(fakes::get_user_def(), fakes::get_user)
        .tool(fakes::list_orders_def(), fakes::list_orders)
        .tool(fakes::get_inventory_def(), fakes::get_inventory)
        .tool(fakes::create_discount_def(), fakes::create_discount)
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
            commands: "get_user --id 1".to_string(),
        })
        .await;
    println!("$ get_user --id 1");
    println!("{}", resp.stdout);

    // ---- Demo 2: Pipeline with jq ----
    println!("--- Demo 2: Pipeline with jq ---");
    let resp = tool
        .execute(ToolRequest {
            commands: "get_user --id 1 | jq -r '.name'".to_string(),
        })
        .await;
    println!("$ get_user --id 1 | jq -r '.name'");
    println!("{}", resp.stdout);

    // ---- Demo 3: Multi-step orchestration ----
    println!("--- Demo 3: Multi-step orchestration ---");
    let script = r#"
        user=$(get_user --id 1)
        name=$(echo "$user" | jq -r '.name')
        tier=$(echo "$user" | jq -r '.tier')
        orders=$(list_orders --user_id 1)
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
            user=$(get_user --id $uid)
            name=$(echo "$user" | jq -r '.name')
            tier=$(echo "$user" | jq -r '.tier')
            if [ "$tier" = "premium" ]; then
                echo "$name is premium - creating discount"
                create_discount --user_id $uid --percent 20 | jq -r '.code'
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
            result=$(get_inventory --item "$item")
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
            name=$(get_user --id $uid | jq -r '.name')
            orders=$(list_orders --user_id $uid)
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

// ---------------------------------------------------------------------------
// Fake e-commerce API — tool definitions + handlers
//
// In a real application these callbacks would call actual HTTP APIs, databases,
// etc. They live in a separate module so the main function can focus on the
// ScriptedTool builder API and the demo scenarios.
// ---------------------------------------------------------------------------
mod fakes {
    use bashkit::{ToolArgs, ToolDef};

    // -- get_user --------------------------------------------------------

    pub fn get_user_def() -> ToolDef {
        ToolDef::new("get_user", "Fetch user by ID").with_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "User ID"}
            },
            "required": ["id"]
        }))
    }

    pub fn get_user(args: &ToolArgs) -> Result<String, String> {
        let id = args.param_i64("id").ok_or("missing --id")?;

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

    // -- list_orders -----------------------------------------------------

    pub fn list_orders_def() -> ToolDef {
        ToolDef::new("list_orders", "List orders for a user").with_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "user_id": {"type": "integer", "description": "User ID"}
            },
            "required": ["user_id"]
        }))
    }

    pub fn list_orders(args: &ToolArgs) -> Result<String, String> {
        let uid = args.param_i64("user_id").ok_or("missing --user_id")?;

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

    // -- get_inventory ---------------------------------------------------

    pub fn get_inventory_def() -> ToolDef {
        ToolDef::new("get_inventory", "Check inventory for an item").with_schema(
            serde_json::json!({
                "type": "object",
                "properties": {
                    "item": {"type": "string", "description": "Item name"}
                },
                "required": ["item"]
            }),
        )
    }

    pub fn get_inventory(args: &ToolArgs) -> Result<String, String> {
        let item = args.param_str("item").ok_or("missing --item")?;

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

    // -- create_discount -------------------------------------------------

    pub fn create_discount_def() -> ToolDef {
        ToolDef::new("create_discount", "Create a discount code").with_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "user_id": {"type": "integer", "description": "User ID"},
                "percent": {"type": "integer", "description": "Discount percentage"}
            },
            "required": ["user_id", "percent"]
        }))
    }

    pub fn create_discount(args: &ToolArgs) -> Result<String, String> {
        let uid = args.param_i64("user_id").ok_or("missing --user_id")?;
        let pct = args.param_i64("percent").ok_or("missing --percent")?;
        Ok(format!(
            "{{\"code\":\"SAVE{pct}-U{uid}\",\"percent\":{pct},\"user_id\":{uid}}}\n"
        ))
    }
}
