extern crate gcc;

use std::{env, fs, path};

fn main() {
    // compile libsph
    gcc::compile_library("libsph_shabal.a", &["lib/shabal.c"]);

    // copy default config to target
    let resource_file = "config.json";
    let source_path = env::current_dir().unwrap().join(resource_file);
    let output_path = path::Path::new(env!("OUT_DIR")).join(resource_file);
    fs::copy(&source_path, &output_path).unwrap();
}