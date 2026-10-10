//! Prints asset diagnostics from real loads through the BRP mailbox, without a GPU.
#![expect(clippy::print_stdout, reason = "This example prints the BRP responses")]

#[path = "support/asset_demo.rs"]
mod asset_demo;

fn main() {
    let mut app = asset_demo::demo_app();
    asset_demo::settle(&mut app);
    for method in ["titan.assets", "titan.asset_failures"] {
        let result = asset_demo::call(&mut app, method, None).unwrap();
        println!(
            "{method}:\n{}",
            serde_json::to_string_pretty(&result).unwrap()
        );
    }
}
