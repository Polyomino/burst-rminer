extern crate rustc_serialize;

use byteorder::*;
use constants::*;
use rustc_serialize::json;
use rustc_serialize::{Decodable, Decoder};
use rustc_serialize::hex::{FromHex, FromHexError};
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
pub enum Error {
    FromHex(FromHexError),
    Http(HyperError),
    Io(IoError),
    Parse(json::DecoderError),
    Subscriber(SendError<miner::MinerWork>),
    MissingWork,
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

#[derive(Clone)]
pub struct Pool {
    url: Url,
    mining_info: Arc<Mutex<Option<MiningInfo>>>,
    subscribers: Arc<Mutex<Vec<Sender<miner::MinerWork>>>>,
    client: Arc<Mutex<Client>>,
}

impl Pool {
    pub fn new(url: Url, miners: Vec<Sender<miner::MinerWork>>) -> Pool {
        let pool = Pool {
            url: url,
            mining_info: Arc::new(Mutex::new(None)),
            subscribers: Arc::new(Mutex::new(miners)),
            client: Arc::new(Mutex::new(Client::new())),
        };
        let pool_ref = pool.clone();
        thread::spawn(move || {
            loop {
                if let Err(e) = pool_ref.refresh() {
                    println!("refresh pool: {:?}", e);
                }
                thread::sleep(Duration::from_secs(5));
            }
        });
        pool
    }

    fn query_pool(&self) -> Result<MiningInfo, Error> {
        let ref http_client = self.client;
        let mut query_url = self.url.clone();
        match query_url.path_segments_mut() {
            Ok(mut path_segments) => {
                path_segments.pop_if_empty().push("burst");
            }
            Err(_) => return Err(Error::Url),
        };
        query_url.query_pairs_mut().append_pair("requestType", "getMiningInfo");
        let mut response = String::new();
        {
            let client_unwrapped = http_client.lock().unwrap();
            let mut res = try!(client_unwrapped.get(query_url).send());
            try!(res.read_to_string(&mut response));
        }
        let mining_info = try!(json::decode(response.as_str()));
        Ok(mining_info)
    }

    fn refresh(&self) -> Result<(), Error> {
        let new_mining_info = try!(self.query_pool());
        match self.mining_info.lock() {
            Err(_) => {
                panic!("Mutex holding the pool state was poisoned. The main thread may have \
                        panicked.")
            }
            Ok(mut mining_info_guard) => {
                if let Some(ref old_mining_info) = *mining_info_guard {
                    if old_mining_info.generation_signature ==
                       new_mining_info.generation_signature {
                        return Ok(()); //no update
                    }
                }
                *mining_info_guard = Some(new_mining_info);
                try!(self.notify_subscribers());
            }
        }
        Ok(())
    }

    fn notify_subscribers(&self) -> Result<(), Error> {
        let miner_work = try!(self.get_miner_work());
        for sender in self.subscribers.lock().unwrap().deref() {
            try!(sender.send(miner_work.clone()))
        }
        Ok(())
    }

    fn get_miner_work(&self) -> Result<miner::MinerWork, Error> {
        let mining_info_guard = self.mining_info.lock().unwrap();
        if let Some(ref mining_info) = *mining_info_guard {
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
                base_target: mining_info.base_target,
            })
        }
        else {
            Err(Error::MissingWork)
        }
    }
}

pub fn submit_hash(nonce: u64, account_id: u64) -> Result<String, Error> {
    let request = format!("http://pool.burst-team.\
                           us/burst?requestType=submitNonce&accountId={}&nonce={}&secretPhrase=cryptoport",
                          account_id,
                          nonce);
    println!("{}", request);
    let client = Client::new();
    let mut response = String::new();
    let mut res = try!(client.get(request.into_url().unwrap()).send());

    // assert_eq!(res.status, hyper::Ok);
    res.read_to_string(&mut response).unwrap();
    return Ok(response);
}
