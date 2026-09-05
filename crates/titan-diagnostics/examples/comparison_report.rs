use std::{env, path::Path};
use titan::render::{Color, Image};
use titan_diagnostics::{ComparisonOptions, write_comparison_report};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = env::args().nth(1).unwrap_or_else(|| "target".into());
    let expected = Image::from_fn(96, 64, |x, y| fixture_pixel(x, y, false))?;
    let actual = Image::from_fn(96, 64, |x, y| fixture_pixel(x, y, true))?;
    let written = write_comparison_report(
        Path::new(&root),
        &expected,
        &actual,
        ComparisonOptions::default(),
    )?;
    println!("{}", written.directory.display());
    Ok(())
}

fn fixture_pixel(x: u32, y: u32, changed: bool) -> Color {
    if changed && (24..48).contains(&x) && (18..42).contains(&y) {
        Color::rgba(235, 70, 55, 210)
    } else {
        Color::rgba((x * 2) as u8, (y * 3) as u8, 120, 255)
    }
}
