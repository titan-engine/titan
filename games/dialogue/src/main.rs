use std::{env, process};

fn main() {
    process::exit(dialogue::tool().run(env::args_os().skip(1)));
}
