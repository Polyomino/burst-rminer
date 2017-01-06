extern crate libc;

use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian, BigEndian};
use constants::*;
use plots::Plot;
use pool;
use rustc_serialize::hex::FromHex;
use std::fs::{File, metadata};
use std::io::{Cursor, Error, Write};
use std::os::unix::io::AsRawFd;
use std::{ptr, slice};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use sph_shabal;

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

impl MinerWork {
    pub fn from_mining_info(mining_info: pool::MiningInfo) -> Result<MinerWork, pool::Error> {
        let sig = try!(mining_info.generation_signature.from_hex());

        let mut height_vec = vec![];
        try!(height_vec.write_u64::<BigEndian>(mining_info.height));
        let height = &height_vec[..];
        let mut scoop_prefix: [u8; 40] = [0; 40];

        try!((&mut scoop_prefix[0..32]).write(&sig[..]));
        try!((&mut scoop_prefix[32..40]).write(height));
        // println!("scoop prefix:    {:?}", scoop_prefix.to_hex());

        let scoop_prefix_shabal = sph_shabal::shabal256(&scoop_prefix);
        // println!("shabaled prefix: {:?}", scoop_prefix_shabal.to_hex());

        let scoop_check_arr = &scoop_prefix_shabal[30..];
        let mut cur = Cursor::new(scoop_check_arr);
        let scoop_num: u16 = cur.read_u16::<BigEndian>().unwrap() % 4096;
        // println!("scoop num:       {:?}", scoop_num);

        let mut hasher: [u8; 32 + HASH_SIZE * 2] = [0; 32 + HASH_SIZE * 2];

        try!((&mut hasher[0..32]).write(&sig[..]));

        Ok(MinerWork {
            hasher: hasher,
            scoop_num: scoop_num,
            height: mining_info.height,
            target_deadline: mining_info.target_deadline,
            base_target: mining_info.base_target,
        })

    }
}

#[cfg(target_arch = "arm")]
use libc::mmap64 as mmap;
#[cfg(target_os = "macos")]
use libc::mmap;

pub fn mine(pool: pool::Pool, signature_recv: Receiver<MinerWork>, plots: Vec<Plot>) {

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

            let scoop_offset = plot.stagger_size as i64 * scoop_num as i64 * HASH_SIZE as i64 * 2;

            let stagger_count = plot.nonce_count / plot.stagger_size;

            let map_len = plot.stagger_size as usize * HASH_SIZE * 2;

            let file = File::open(&plot.path).unwrap();
            let file_len = metadata(&plot.path).unwrap().len();

            for stagger in 0..stagger_count {
                let stagger_offset = (stagger as i64 * HASH_CAP as i64 * HASH_SIZE as i64 * 2 *
                                      plot.stagger_size as i64 +
                                      scoop_offset) as i64;
                let alignment = stagger_offset % page_size();
                let aligned_offset = stagger_offset - alignment;
                let aligned_len = map_len + alignment as usize;
                if map_len as u64 + stagger_offset as u64 > file_len {
                    println!("past end of file {:?}", &plot.path);
                    break;
                }
                
                let map_addr;
                let buf: &[u8] = unsafe {
                    map_addr = mmap(ptr::null_mut(),
                                    aligned_len,
                                    libc::PROT_READ,
                                    libc::MAP_PRIVATE,
                                    file.as_raw_fd(),
                                    aligned_offset);
                    if map_addr == libc::MAP_FAILED {
                        println!("map failed: {}",
                                 Error::last_os_error().raw_os_error().unwrap_or(-1));
                        continue;
                    }
                    slice::from_raw_parts(map_addr.offset(alignment as isize) as *const u8, map_len)
                };

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
                    match best_hash {
                        Some(existing_hash) if existing_hash <= test_num => {}
                        _ => {
                            best_hash = Some(test_num);
                            best_nonce = Some(nonce);
                            best_account_id = Some(plot.account_id);
                        }
                    }

                    // println!("{:?}", best_hash);
                    nonce_count += 1;
                    nonce += 1;
                }
                unsafe { libc::munmap(map_addr, map_len) };

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
                            match pool.submit_hash(best_nonce.unwrap(), best_account_id.unwrap()) {
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

fn page_size() -> i64 {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as i64 }
}