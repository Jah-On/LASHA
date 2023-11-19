// build.rs
fn main() {
    println!("lol");
    cc::Build::new()
        .file("./src/g722_encode.c")
        .compile("g722_encode");
}
