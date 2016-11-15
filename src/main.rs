extern crate byteorder;
extern crate regex;
extern crate rustc_serialize;
extern crate libc;
extern crate hyper;

mod config;
mod constants;
mod miner;
mod plots;
mod pool;
mod sph_shabal;

use byteorder::*;
use constants::*;
use miner::MinerResult;
use regex::Regex;
use rustc_serialize::json;
use rustc_serialize::hex::FromHex;
use rustc_serialize::hex::ToHex;
use std::cmp::Ordering;
use std::env;
use std::fs::File;
use std::io::Cursor;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::thread;

fn main() {
    let args: Vec<String> = env::args().collect();
    let res: Result<PathBuf, &str> = match args.len().cmp(&1) {
        Ordering::Greater => {
            let re = Regex::new(r"^-config=(.*)").unwrap();
            match re.captures(&args[1]) {
                Some(captures) => Ok(PathBuf::from(&captures[1])),
                None => Err("Failed to find path"),
            }
        }
        _ => Ok(PathBuf::from("./config.json")),
    };
    let config_path = res.unwrap();
    if !config_path.exists() {
        usage();
        std::process::exit(1);
    }

    let mut config_file = File::open(config_path).unwrap();

    let mut data = String::new();

    config_file.read_to_string(&mut data).unwrap();
    let miner_config = json::decode::<config::MinerConfiguration>(&data).unwrap();
    // println!("found config!");
    // println!("pool_url: {:?}", miner_config.pool_url);
    // println!("plot_folders: {:?}", miner_config.plot_folders);

    let plot_folders = plots::get_plots(miner_config.plot_folders.unwrap());

    // for folder in &plot_folders.folders {
    //     // println!("folder name {:?}", folder.path);
    //     for file in &folder.plots {
    //         // println!("file: {:?}", file.path);
    //         // println!("account_id: {}", file.account_id);
    //         // println!("start_nonce: {}", file.start_nonce);
    //     }
    // }
    // read_file(&plot_folders.folders[0].plots[0].path);

    // let mut input: [u64; 2] = [0x9EAEA6EE9400A3D3, 0x0000000000000000];
    // let plot = generate_plot(input);


    let (result_sender, result_recv) = channel();
    let mut miners = Vec::new();
    for folder in &plot_folders.folders {
        let plots = &folder.plots;
        let threads_per_folder = miner_config.threads_per_folder.unwrap() as usize;
        let mut plots_per_thread = plots.len() / threads_per_folder;
        let plots_per_thread_rem = plots.len() % threads_per_folder;
        if plots_per_thread_rem != 0 {
            plots_per_thread += 1;
        }
        for plots_chunk in plots.chunks(plots_per_thread) {
            let plots_vec = plots_chunk.to_vec();
            let (signature_sender, signature_recv) = channel();
            let result_sender = result_sender.clone();
            miners.push(miner::Miner {
                thread: thread::spawn::<_, i32>(move || {
                    miner::mine(result_sender, signature_recv, plots_vec);
                    0
                }),
                work_sender: signature_sender,
            })
        }
    }

    thread::spawn(|| pool::poll_pool(miners));

    let thread_count = plot_folders.folders.len() as u32 * miner_config.threads_per_folder.unwrap();

    let mut height = 0;
    let mut best_result: Option<MinerResult> = None;

    let mut result_count = 0;
    loop {
        let result: MinerResult = result_recv.recv().unwrap();
        if result.height != height {
            height = result.height;
            result_count = 0;
        }
        best_result = match best_result {
            Some(x) => {
                if x.hash < result.hash {
                    Some(x)
                } else {
                    Some(result)
                }
            }
            None => Some(result),
        };
        result_count += 1;
        if result_count >= thread_count {
            println!("{}",
                     pool::submit_hash(best_result.unwrap().nonce,
                                       best_result.unwrap().account_id));
        }
    }
}

fn usage() {
    println!("rust-miner [-config={{ path_to_config }}");
}

// fn read_file(path: &PathBuf) -> Result<(), std::io::Error> {
//     let file = try!(File::open(path));
//     let mut reader = BufReader::new(file);
//     let mut buffer = [0u8; 32];
//     let i = 5;
//     for x in 0..i {
//         match reader.read(&mut buffer) {
//             Ok(size) => {
//                 if size < 32 {
//                     return Err(std::io::Error::new(std::io::ErrorKind::Other, "oh no"));
//                 }
//             }
//             Err(out) => return Err(out),
//         }
//     }
//     println!("read file");//offset:{} {:?}",i, buffer);
//     Ok(())
// }