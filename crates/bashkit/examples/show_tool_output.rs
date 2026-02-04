use bashkit::{BashTool, Tool};

fn main() {
    let tool = BashTool::default();

    println!("=== name() ===");
    println!("{}", tool.name());

    println!("\n=== short_description() ===");
    println!("{}", tool.short_description());

    println!("\n=== description() ===");
    println!("{}", tool.description());

    println!("\n=== system_prompt() ===");
    println!("{}", tool.system_prompt());

    println!("\n=== llmtext() ===");
    println!("{}", tool.llmtext());

    println!("\n=== version() ===");
    println!("{}", tool.version());
}
