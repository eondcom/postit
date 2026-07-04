//! Prints the enumerated outputs so `outputs.rs` can be verified without the
//! GUI: `cargo run --example outputs_probe`

#[path = "../src/outputs.rs"]
mod outputs;

fn main() {
    let list = outputs::list_outputs();
    println!("{} output(s)", list.len());
    for o in &list {
        println!("{} pos=({},{}) size={}x{}", o.name, o.x, o.y, o.width, o.height);
    }
}
