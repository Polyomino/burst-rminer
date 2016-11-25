use byteorder::{ReadBytesExt, LittleEndian};
use constants::*;
use plots::Plot;
use std::cmp::Ordering;
use std::fs::File;
use std::io::{Cursor, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;
use sph_shabal;
use memmap::{Mmap, Protection};

pub struct Miner {
    pub thread: JoinHandle<i32>,
    pub work_sender: Sender<MinerWork>,
}

pub struct MinerWork {
    pub hasher: [u8; 32 + HASH_SIZE * 2],
    pub scoop_num: u16,
    pub height: u64,
}

#[derive(Clone,Copy)]
pub struct MinerResult {
    pub nonce: u64,
    pub account_id: u64,
    pub hash: u64,
    pub height: u64,
}

pub fn mine(result_sender: Sender<MinerResult>,
            signature_recv: Receiver<MinerWork>,
            plots: Vec<Plot>) {
    loop {
        // println!("start mine loop");
        let miner_work = signature_recv.recv().unwrap();

        let mut hasher = miner_work.hasher;
        let scoop_num = miner_work.scoop_num;
        let height = miner_work.height;
        let start_time = Instant::now();

        let mut best_nonce: Option<u64> = None;
        let mut best_account_id: Option<u64> = None;
        let mut best_hash: Option<u64> = None;

        for plot in &plots {
            println!("read file: {:?}", &plot.path);

            let scoop_offset = plot.stagger_size as usize * scoop_num as usize * HASH_SIZE * 2;

            let mut nonce = plot.start_nonce;

            let stagger_count = plot.nonce_count / plot.stagger_size;

            let file = File::open(&plot.path).unwrap();
            for stagger in 0..stagger_count {
                let stagger_offset = stagger as usize * HASH_CAP * HASH_SIZE * 2 *
                                     plot.stagger_size as usize +
                                     scoop_offset;
                let mmap_stagger = Mmap::open_with_offset(&file,
                                                          Protection::Read,
                                                          stagger_offset,
                                                          plot.stagger_size as usize * HASH_SIZE *
                                                          2)
                    .unwrap();
                let buf = unsafe { mmap_stagger.as_slice() };
                for nonce_in_stagger in 0..plot.stagger_size {
                    (& mut hasher[32..(32 + HASH_SIZE * 2)])
                        .write(&buf[nonce_in_stagger as usize * HASH_SIZE * 2..(nonce_in_stagger as usize + 1) *
                                                                     HASH_SIZE *
                                                                     2])
                        .unwrap();
                    let outhash = sph_shabal::shabal256(&hasher);
                    let mut hash_cur = Cursor::new(&outhash[0..8]);
                    let test_num = hash_cur.read_u64::<LittleEndian>().unwrap();
                    // println!("hash: {} nonce: {}", test_num, nonce);
                    best_hash = match (best_hash, Some(test_num).cmp(&best_hash)) {
                        (None, _) |
                        (Some(_), Ordering::Less) => {
                            best_nonce = Some(nonce);
                            best_account_id = Some(plot.account_id);
                            Some(test_num)
                        }
                        _ => best_hash,
                    };
                    println!("{:?}", best_hash);
                    nonce += 1;
                }
            }
        }
        result_sender.send(MinerResult {
                account_id: best_account_id.unwrap(),
                hash: best_hash.unwrap(),
                nonce: best_nonce.unwrap(),
                height: height,
            })
            .unwrap();
        println!("finished reading in {:?}", Instant::now() - start_time);
    }
}
