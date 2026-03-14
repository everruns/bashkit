use bashkit::{BashTool, Tool};

fn main() {
    let builder = BashTool::builder().username("agent").hostname("sandbox");
    let tool = builder.build();

    println!("=== name() ===");
    println!("{}", tool.name());

    println!("\n=== display_name() ===");
    println!("{}", tool.display_name());

    println!("\n=== short_description() ===");
    println!("{}", tool.short_description());

    println!("\n=== description() ===");
    println!("{}", tool.description());

    println!("\n=== system_prompt() ===");
    println!("{}", tool.system_prompt());

    println!("\n=== help() ===");
    println!("{}", tool.help());

    println!("\n=== input_schema() ===");
    println!("{}", tool.input_schema());

    println!("\n=== output_schema() ===");
    println!("{}", tool.output_schema());

    println!("\n=== build_tool_definition() ===");
    println!("{}", builder.build_tool_definition());

    println!("\n=== version() ===");
    println!("{}", tool.version());
}
