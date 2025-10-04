use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct Example {
    message: String,
}

fn main() {
    println!("Rust example with transient dependencies");
}
