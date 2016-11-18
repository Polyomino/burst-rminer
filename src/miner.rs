use byteorder::{ReadBytesExt, LittleEndian};
use constants::*;
use plots::Plot;
use std::fs::File;
use std::io::{Cursor, Read, BufReader, Seek, SeekFrom};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;
use sph_shabal;

pub struct Miner {
    pub thread: JoinHandle<i32>,
    pub work_sender: Sender<MinerWork>,
}

pub struct MinerWork {
    pub hasher: [u8; 32 + HASH_SIZE * 2],
    pub scoop_num: u16,
    pub height: u64,
}

struct HasherWork {
    hasher: [u8; 32 + HASH_SIZE * 2],
    height: u64,
    account_id: u64,
    nonce: u64,
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
            plots: Vec<Plot>,
            threads_per_folder: u64) {
    let nonce_count: u64 = plots.iter().map(|plot| plot.nonce_count).sum();
    let mut hashers = vec![];
    for _hashulator in 0..threads_per_folder {
        let (work_sender, work_reciever) = channel();
        let result_sender_clone = result_sender.clone();
        thread::spawn(move || {
            hashulator(result_sender_clone,
                       work_reciever,
                       nonce_count / threads_per_folder);
        });
        hashers.push(work_sender);
    }
    loop {
        // println!("start mine loop");
        let miner_work = signature_recv.recv().unwrap();

        let mut hasher = miner_work.hasher;
        let scoop_num = miner_work.scoop_num;
        let start_time = Instant::now();

        for plot in &plots {
            println!("read file: {:?}", &plot.path);
            let file = File::open(&plot.path).unwrap();
            let mut reader = BufReader::new(file);

            let offset = plot.stagger_size * (scoop_num as u64) * (HASH_SIZE as u64) * 2;

            let mut nonce = plot.start_nonce;

            let stagger_count = plot.nonce_count / plot.stagger_size;
            for stagger in 0..stagger_count {
                let cur_offset = (HASH_CAP as u64) * 2 * stagger * plot.stagger_size + offset;
                reader.seek(SeekFrom::Start(cur_offset)).unwrap();
                for _nonce_in_stagger in 0..plot.stagger_size {
                    if let Err(err) = reader.read_exact(&mut hasher[32..(32 + HASH_SIZE * 2)]) {
                        println!("error reading file {:?}: {:?}", &plot.path, err);
                        break;
                    }
                    hashers[(nonce % threads_per_folder) as usize]
                        .send(HasherWork {
                            hasher: hasher,
                            height: miner_work.height,
                            account_id: plot.account_id,
                            nonce: nonce,
                        })
                        .unwrap();

                    nonce += 1;
                }
            }
        }
        println!("finished reading in {:?}", Instant::now() - start_time);
    }
}

fn hashulator(result_sender: Sender<MinerResult>,
              work_reciever: Receiver<HasherWork>,
              nonce_count: u64) {
    let mut best_nonce: Option<u64> = None;
    let mut best_account_id: Option<u64> = None;
    let mut best_hash: Option<u64> = None;
    let mut height: Option<u64> = None;

    let mut nonces_left = nonce_count;
    loop {
        let work = work_reciever.recv().unwrap();

        if let Some(old_height) = height {
            if old_height != work.height {
                height = Some(work.height);
            }
        }

        let outhash = sph_shabal::shabal256(&work.hasher);
        let mut hash_cur = Cursor::new(&outhash[0..8]);
        let test_num = hash_cur.read_u64::<LittleEndian>().unwrap();
        best_hash = match best_hash {
            Some(hash) if test_num < hash => {
                best_nonce = Some(work.nonce);
                best_account_id = Some(work.account_id);
                Some(test_num)
            }
            Some(_) => best_hash,
            None => {
                best_nonce = Some(work.nonce);
                best_account_id = Some(work.account_id);
                Some(test_num)
            }
        };
        nonces_left -= 1;
        if nonces_left == 0 {
            result_sender.send(MinerResult {
                    account_id: best_account_id.unwrap(),
                    hash: best_hash.unwrap(),
                    nonce: best_nonce.unwrap(),
                    height: work.height,
                })
                .unwrap();
            nonces_left = nonce_count;
        }
    }
}
