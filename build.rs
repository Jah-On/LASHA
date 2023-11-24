// build.rs
fn main() {
    cc::Build::new()
        .file("./include/g722_encode.c")
        .compile("g722_encode");
}
