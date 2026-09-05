use titan::{Component, Inspect};

#[derive(Component, Inspect)]
struct InvalidBounds {
    #[inspect(writable, minimum = 0, maximum = 1)]
    enabled: bool,
}

fn main() {}
