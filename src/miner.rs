use byteorder::ReadBytesExt;
use byteorder::LittleEndian;
use constants::*;
use plots::Plot;
use std::fs::File;
use std::io::Cursor;
use std::io::Read;
use std::io::BufReader;
use std::io::Seek;
use std::io::SeekFrom;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
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
        //println!("start mine loop");
        let miner_work = signature_recv.recv().unwrap();

        let mut hasher = miner_work.hasher;
        let scoop_num = miner_work.scoop_num;
        let start_time = Instant::now(); 

        let mut best_nonce: Option<u64> = None;
        let mut best_account_id: Option<u64> = None;
        let mut best_hash: Option<u64> = None;

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
                    reader.read_exact(&mut hasher[32..(32 + HASH_SIZE * 2)]).unwrap();
                    // println!("nonce: {}, offset:{}, hasher: {:?}",
                    //          nonce,
                    //          cur_offset,
                    //          &hasher.to_hex());
                    let outhash = sph_shabal::shabal256(&hasher);
                    // println!("outhash: {:?}", outhash.to_hex());
                    let mut hash_cur = Cursor::new(&outhash[0..8]);
                    let test_num = hash_cur.read_u64::<LittleEndian>().unwrap();
                    // let test_num = BigUint::from_bytes_le(&outhash);
                     best_hash = match best_hash {
                        Some(hash) => {
                            if test_num < hash {                     mining_info.base_target.unwrap()));
                                best_nonce = Some(nonce);
                                best_account_id = Some(plot.account_id);
                                Some(test_num)
                            } else {
                                Some(hash)
                            }
                        }
                        None => {
                            best_nonce = Some(nonce);
                            best_account_id = Some(plot.account_id);
                            Some(test_num)
                        }
                    };
                    nonce += 1;
                }
            }
        }
        println!("finished in {:?}", Instant::now() - start_time);
        result_sender.send(MinerResult {
                account_id: best_account_id.unwrap(),
                hash: best_hash.unwrap(),
                nonce: best_nonce.unwrap(),
                height: miner_work.height,
            })
            .unwrap();
    }
}