extern crate gcc;

fn main() {
    gcc::compile_library("libsph_shabal.a", &["lib/shabal.c"]);
}