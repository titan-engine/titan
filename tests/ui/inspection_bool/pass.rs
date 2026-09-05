use titan::{Component, Inspect};

#[derive(Component, Inspect)]
struct Flags {
    /// A writable boolean exposed to inspection.
    #[inspect(writable)]
    enabled: bool,
    /// A read-only boolean exposed to inspection.
    #[inspect]
    visible: bool,
}

fn main() {}
