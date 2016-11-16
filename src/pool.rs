extern crate rustc_serialize;

use byteorder::*;
use constants::*;
use libc;
use rustc_serialize::json::Json;
use rustc_serialize::hex::FromHex;
use rustc_serialize::hex::ToHex;
use hyper;
use hyper::client::Client;
use hyper::error::Error;
use hyper::client::IntoUrl;
use std::thread;
use std::time::Duration;
use std::io::Read;
use std::io::Cursor;
use sph_shabal;
use miner;

#[derive(Debug)]
struct MiningInfo {
    pub generation_signature: Option<String>,
    pub base_target: Option<u64>,
    request_processing_time: Option<i64>,
    height: Option<u64>,
    target_deadline: Option<i64>,
}

pub fn poll_pool(miners: Vec<miner::Miner>) -> () {
    println!("poll pool");
    let mut old_signature: Option<String> = None;

    loop {
        let res = get_mining_info();
        if res.is_err() {
            thread::sleep(Duration::from_secs(5));
            continue;
        }
        let mining_info = res.unwrap();

        let signature = mining_info.generation_signature.as_ref().unwrap();
        old_signature = match old_signature {
            Some(ref old_sig) if old_sig != signature => Some(signature.clone()),
            None => Some(signature.clone()),
            _ => {
                thread::sleep(Duration::from_secs(5));
                continue;
                // old_signature
            }
        };

        println!("{:?}", &mining_info);

        let sig = signature.from_hex().unwrap();
        println!("arr:{:?} len:{}", &sig.to_hex(), sig.len());

        let mut height_vec = vec![];
        height_vec.write_u64::<BigEndian>(mining_info.height.unwrap()).unwrap();
        let height = &height_vec[..];
        let mut scoop_prefix: [u8; 40] = [0; 40];

        unsafe {
            libc::memcpy(&mut scoop_prefix[0] as *mut _ as *mut libc::c_void,
                         &sig[..] as *const _ as *const libc::c_void,
                         32 as libc::size_t);
            libc::memcpy(&mut scoop_prefix[32] as *mut _ as *mut libc::c_void,
                         height as *const _ as *const libc::c_void,
                         8 as libc::size_t);
        }
        println!("scoop prefix:    {:?}", scoop_prefix.to_hex());

        let scoop_prefix_shabal = sph_shabal::shabal256(&scoop_prefix);
        println!("shabaled prefix: {:?}", scoop_prefix_shabal.to_hex());

        let scoop_check_arr = &scoop_prefix_shabal[30..];
        let mut cur = Cursor::new(scoop_check_arr);
        let scoop_num: u16 = cur.read_u16::<BigEndian>().unwrap() % 4096;
        println!("scoop num:       {:?}", scoop_num);

        let mut hasher: [u8; 32 + HASH_SIZE * 2] = [0; 32 + HASH_SIZE * 2];
        unsafe {
            libc::memcpy(&mut hasher as *mut _ as *mut libc::c_void,
                         &sig[..] as *const _ as *const libc::c_void,
                         32 as libc::size_t);
        }
        println!("hasher: {:?}", &hasher.to_hex());

        for miner in &miners {
            miner.work_sender
                .send(miner::MinerWork {
                    hasher: hasher,
                    scoop_num: scoop_num,
                    height: mining_info.height.unwrap(),
                })
                .unwrap();
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn get_mining_info() -> Result<MiningInfo, Error> {
    let client = Client::new();
    let mut res = try!(client.get("http://pool.burst-team.us/burst?requestType=getMiningInfo")
        .send());
    assert_eq!(res.status, hyper::Ok);
    let mut response = String::new();
    res.read_to_string(&mut response).unwrap();
    let json = Json::from_str(response.as_str()).unwrap();
    let json_obj = json.as_object().unwrap();
    // println!("{:?}", response);
    Ok(MiningInfo {
        generation_signature: Some(json_obj.get("generationSignature")
            .unwrap()
            .as_string()
            .unwrap()
            .to_string()),
        base_target: Some(json_obj.get("baseTarget")
            .unwrap()
            .as_string()
            .unwrap()
            .to_string()
            .parse::<u64>()
            .unwrap()),
        request_processing_time: json_obj.get("requestProcessingTime").unwrap().as_i64(),
        height: Some(json_obj.get("height")
            .unwrap()
            .as_string()
            .unwrap()
            .to_string()
            .parse::<u64>()
            .unwrap()),
        target_deadline: json_obj.get("targetDeadline").unwrap().as_i64(),
    })
}

pub fn submit_hash(nonce: u64, account_id: u64) -> String {
    let request = format!("http://pool.burst-team.\
                           us/burst?requestType=submitNonce&accountId={}&nonce={}&secretPhrase=cryptoport",
                          account_id,
                          nonce);
    println!("{}", request);
    let client = Client::new();
    let mut res = client.get(request.into_url().unwrap()).send().unwrap();
    // assert_eq!(res.status, hyper::Ok);
    let mut response = String::new();
    res.read_to_string(&mut response).unwrap();
    return response;
}
