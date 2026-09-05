#[path = "src/generator.rs"]
mod generator;

fn main() {
    println!("cargo:rerun-if-changed=src/generator.rs");
    println!("cargo:rerun-if-changed=build.rs");
    // An evidence harness may force the actual build script to execute again.
    println!("cargo:rerun-if-env-changed=TITAN_ASSET_BUILD_CHECK");
    let directory = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mut generator = generator::Generator::default();
    let asset = generator::load(
        &directory.join("generated-cache"),
        generator::Inputs::default(),
        &mut generator,
    )
    .expect("generate fixture build-time PNG");
    std::fs::write(directory.join("generated.png"), &asset.png).expect("write embedded PNG");
    println!(
        "cargo:rustc-env=TITAN_ASSET_BUILD_OUTCOME={}",
        asset.outcome.as_str()
    );
    println!(
        "cargo:rustc-env=TITAN_ASSET_BUILD_GENERATIONS={}",
        generator.generation_count
    );
}
