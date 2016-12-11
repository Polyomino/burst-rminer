use byteorder::{ReadBytesExt, LittleEndian};
use constants::*;
use plots::Plot;
use pool;
use std::cmp::Ordering;
use std::fs::File;
use std::io::{Cursor, Write};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use sph_shabal;
use memmap::{Mmap, Protection};

#[derive(Copy)]
pub struct MinerWork {
    pub hasher: [u8; 32 + HASH_SIZE * 2],
    pub scoop_num: u16,
    pub height: u64,
    pub target_deadline: u64,
    pub base_target: u64,
}

impl Clone for MinerWork {
    fn clone(&self) -> MinerWork {
        *self
    }
}

pub fn mine(signature_recv: Receiver<MinerWork>, plots: Vec<Plot>) {

    let mut next_work: Option<MinerWork> = None;

    loop {
        // println!("start mine loop");
        let miner_work = match next_work {
            Some(t) => t,
            None => signature_recv.recv().unwrap(),
        };

        let mut hasher = miner_work.hasher;
        let scoop_num = miner_work.scoop_num;
        let start_time = Instant::now();

        let deadline = miner_work.target_deadline * miner_work.base_target;

        let mut nonce_count = 0;
        let mut last_submit: Option<u64> = None;
        let mut best_nonce: Option<u64> = None;
        let mut best_account_id: Option<u64> = None;
        let mut best_hash: Option<u64> = None;

        'miner_run: for plot in &plots {
            // println!("read file: {:?}", &plot.path);
            let plot_start_time = Instant::now();
            let mut last_check_time = plot_start_time;
            let mut nonce = plot.start_nonce;

            let scoop_offset = plot.stagger_size as usize * scoop_num as usize * HASH_SIZE * 2;

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

                    // println!("{:?}", best_hash);
                    nonce_count += 1;
                    nonce += 1;
                }
                let time_since_check = Instant::now() - last_check_time;
                if time_since_check > Duration::from_millis(500) {
                    last_check_time = Instant::now();
                    if has_new_signature(&signature_recv, &mut next_work) {
                        println!("read {} nonces in {:?}", nonce_count, time_since_check);
                        break 'miner_run;
                    }

                    if best_hash.unwrap() < deadline && best_nonce != last_submit {
                        println!("found nonce {} Duration: {:?}",
                                 best_nonce.unwrap(),
                                 Duration::from_secs(best_hash.unwrap() / miner_work.base_target));
                        for i in 0..3 {
                            match pool::submit_hash(best_nonce.unwrap(),
                                                    best_account_id.unwrap()) {
                                Ok(t) => {
                                    println!("try {} pool response: {}", i, t);
                                    last_submit = best_nonce;
                                    break;
                                }
                                Err(e) => println!("try {} pool error: {:?}", i, e),
                            };
                        }
                    }
                }
            }
        }
        println!("finished reading in {:?}", Instant::now() - start_time);
    }
}

fn has_new_signature(recv: &Receiver<MinerWork>, next_work: &mut Option<MinerWork>) -> bool {
    return match recv.try_recv() {
        Ok(t) => {
            *next_work = Some(t);
            true
        }
        Err(TryRecvError::Empty) => {
            *next_work = None;
            false
        }
        Err(TryRecvError::Disconnected) => panic!("signature sender disconnected"),
    };
}