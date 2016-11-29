extern crate byteorder;
extern crate regex;
extern crate rustc_serialize;
extern crate libc;
extern crate hyper;
extern crate memmap;

mod config;
mod constants;
mod miner;
mod plots;
mod pool;
mod sph_shabal;

use hyper::Url;
use miner::MinerResult;
use pool::Pool;
use regex::Regex;
use rustc_serialize::json;
use std::cmp::Ordering;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

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
    println!("found config!");
    println!("pool_url: {:?}", miner_config.pool_url);
    println!("plot_folders: {:?}", miner_config.plot_folders);

    let plot_folders = plots::get_plots(miner_config.plot_folders.unwrap());

    let (result_sender, result_recv) = channel();
    let mut miners = Vec::new();
    let mut senders = Vec::new();
    for folder in &plot_folders.folders {
        let plots = folder.plots.clone();

        let (signature_sender, signature_recv) = channel();
        let result_sender = result_sender.clone();
        miners.push(
            thread::spawn::<_, i32>(move || {
                miner::mine(result_sender, signature_recv, plots);
                0
        }));
        senders.push(signature_sender);
    }

    let pool = Pool::new(Url::parse(&miner_config.pool_url.unwrap()).unwrap(),
                         senders);

    let thread_count = plot_folders.folders.len();

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
            Some(x) if x.hash < result.hash => Some(x),
            _ => Some(result),
        };
        result_count += 1;
        if result_count >= thread_count {
            {
                let base_target = pool.base_target();
                println!("base_target {}", base_target);
                println!("best: {:?}",
                         Duration::from_secs(best_result.unwrap().hash / base_target));
            }
            println!("{}",
                     pool::submit_hash(best_result.unwrap().nonce,
                                       best_result.unwrap().account_id));
        }
    }
}

fn usage() {
    println!("rust-miner [-config={{ path_to_config }}");
}
