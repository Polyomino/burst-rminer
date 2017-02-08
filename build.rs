extern crate gcc;

fn main() {
    // compile libsph
    gcc::compile_library("libsph_shabal.a", &["lib/shabal.c"]);
}