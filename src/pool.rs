extern crate rustc_serialize;

use byteorder::*;
use constants::*;
use rustc_serialize::json;
use rustc_serialize::{Decodable, Decoder};
use rustc_serialize::hex::{FromHex, FromHexError};
use hyper;
use hyper::Url;
use hyper::client::Client;
use hyper::error::Error as HyperError;
use hyper::client::IntoUrl;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Sender, SendError};
use std::thread;
use std::time::Duration;
use std::io::{Cursor, Read, Write, Error as IoError};
use std::ops::Deref;
use sph_shabal;
use miner;

#[derive(Debug, Clone)]
struct MiningInfo {
    pub generation_signature: String,
    pub base_target: u64,
    request_processing_time: i64,
    height: u64,
    target_deadline: u64,
}

impl Decodable for MiningInfo {
    fn decode<D: Decoder>(d: &mut D) -> Result<MiningInfo, D::Error> {
        // 5 is the number of elements in MiningInfo
        d.read_struct("MiningInfo", 5, |d| {
            let generation_signature =
                try!(d.read_struct_field("generationSignature", 0, |d| d.read_str()));
            let base_target = try!(d.read_struct_field("baseTarget", 1, |d| d.read_u64()));
            let request_processing_time =
                try!(d.read_struct_field("requestProcessingTime", 2, |d| d.read_i64()));
            let height = try!(d.read_struct_field("height", 3, |d| d.read_u64()));
            let target_deadline = try!(d.read_struct_field("targetDeadline", 4, |d| d.read_u64()));

            Ok(MiningInfo {
                generation_signature: generation_signature,
                base_target: base_target,
                request_processing_time: request_processing_time,
                height: height,
                target_deadline: target_deadline,
            })
        })


    }
}

#[derive(Debug)]
enum Error {
    FromHex(FromHexError),
    Http(HyperError),
    Io(IoError),
    Parse(json::DecoderError),
    Subscriber(SendError<miner::MinerWork>),
    Url,
}

impl From<HyperError> for Error {
    fn from(err: HyperError) -> Error {
        Error::Http(err)
    }
}

impl From<json::DecoderError> for Error {
    fn from(err: json::DecoderError) -> Error {
        Error::Parse(err)
    }
}

impl From<IoError> for Error {
    fn from(err: IoError) -> Error {
        Error::Io(err)
    }
}

impl From<FromHexError> for Error {
    fn from(err: FromHexError) -> Error {
        Error::FromHex(err)
    }
}

impl From<SendError<miner::MinerWork>> for Error {
    fn from(err: SendError<miner::MinerWork>) -> Error {
        Error::Subscriber(err)
    }
}

pub struct Pool {
    url: Url,
    mining_info: Arc<Mutex<Option<MiningInfo>>>,
    subscribers: Arc<Mutex<Vec<Sender<miner::MinerWork>>>>,
}

impl Pool {
    pub fn new(url: Url, miners: Vec<Sender<miner::MinerWork>>) -> Pool {
        let pool = Pool {
            url: url,
            mining_info: Arc::new(Mutex::new(None)),
            subscribers: Arc::new(Mutex::new(miners)),
        };
        let mining_info = pool.mining_info.clone();
        let subscribers = pool.subscribers.clone();
        let host_url = pool.url.clone();
        thread::spawn(move || {
            let mining_info = mining_info;
            let subscribers = subscribers;
            loop {
                if let Err(e) = Pool::refresh(&mining_info, &subscribers, &host_url) {
                    println!("{:?}", e);
                }
                thread::sleep(Duration::from_secs(5));
            }
        });
        pool
    }

    pub fn base_target(&self) -> u64 {
        self.mining_info.lock().unwrap().clone().unwrap().base_target
    }

    fn query_pool(url: &Url) -> Result<MiningInfo, Error> {
        let http_client = Client::new();
        let mut query_url = url.clone();
        match query_url.path_segments_mut() {
            Ok(mut path_segments) => {
                path_segments.pop_if_empty().push("burst");
            }
            Err(_) => return Err(Error::Url),
        };
        query_url.query_pairs_mut().append_pair("requestType", "getMiningInfo");
        let mut res = try!(http_client.get(query_url).send());
        assert_eq!(res.status, hyper::Ok);
        let mut response = String::new();
        try!(res.read_to_string(&mut response));
        let mining_info = try!(json::decode(response.as_str()));
        Ok(mining_info)
    }

    fn refresh(mining_info: &Arc<Mutex<Option<MiningInfo>>>,
               subscribers: &Arc<Mutex<Vec<Sender<miner::MinerWork>>>>,
               url: &Url)
               -> Result<(), Error> {
        let new_mining_info = try!(Pool::query_pool(url));
        let new_sig = new_mining_info.generation_signature.clone();
        match mining_info.lock() {
            Err(_) => {
                panic!("Mutex holding the pool state was poisoned. The main thread may have \
                        panicked.")
            }
            Ok(mut mining_info_guard) => {
                *mining_info_guard = match mining_info_guard.clone() {
                    None => {
                        try!(Pool::notify_subscribers(&new_mining_info, subscribers));
                        Some(new_mining_info)
                    }
                    Some(ref existing_mining_info) if existing_mining_info.generation_signature !=
                                                      new_sig => {
                        try!(Pool::notify_subscribers(&new_mining_info, subscribers));
                        Some(new_mining_info)
                    }
                    Some(existing_mining_info) => Some(existing_mining_info),
                }
            }
        }
        Ok(())
    }

    fn notify_subscribers(mining_info: &MiningInfo,
                          subscribers: &Arc<Mutex<Vec<Sender<miner::MinerWork>>>>)
                          -> Result<(), Error> {
        let miner_work = try!(Pool::get_miner_work(mining_info));
        for sender in subscribers.lock().unwrap().deref() {
            try!(sender.send(miner_work.clone()))
        }
        Ok(())
    }

    fn get_miner_work(mining_info: &MiningInfo) -> Result<miner::MinerWork, Error> {
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

        Ok(miner::MinerWork {
            hasher: hasher,
            scoop_num: scoop_num,
            height: mining_info.height,
            target_deadline: mining_info.target_deadline,
        })
    }
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
