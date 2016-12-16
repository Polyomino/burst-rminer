extern crate rustc_serialize;

use rustc_serialize::json;
use rustc_serialize::{Decodable, Decoder};
use rustc_serialize::hex::FromHexError;
use hyper::Url;
use hyper::client::Client;
use hyper::error::Error as HyperError;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Sender, SendError};
use std::thread;
use std::time::Duration;
use std::io::{Read, Error as IoError};
use std::ops::Deref;
use miner;

#[derive(Debug, Clone)]
pub struct MiningInfo {
    pub generation_signature: String,
    pub base_target: u64,
    request_processing_time: i64,
    pub height: u64,
    pub target_deadline: u64,
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
    started: Arc<Mutex<bool>>,
}

impl Pool {
    pub fn from_url(url: Url) -> Pool {
        Pool {
            url: url,
            mining_info: Arc::new(Mutex::new(None)),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            client: Arc::new(Mutex::new(Client::new())),
            started: Arc::new(Mutex::new(false)),
        }

    }

    pub fn start(&self) {
        let mut started_mutex_guard = self.started.lock().unwrap();
        if *started_mutex_guard == false {
            let pool_ref = self.clone();
            thread::spawn(move || {
                loop {
                    if let Err(e) = pool_ref.refresh() {
                        println!("refresh pool: {:?}", e);
                    }
                    thread::sleep(Duration::from_secs(5));
                }
            });
        }
        *started_mutex_guard = true;
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
                *mining_info_guard = Some(new_mining_info.clone());
                try!(self.notify_subscribers(new_mining_info));
            }
        }
        Ok(())
    }

    pub fn add_subscriber(&self, subscriber: Sender<miner::MinerWork>) -> Result<(), Error> {
        let mut subs = self.subscribers.lock().unwrap();
        subs.push(subscriber);
        Ok(())
    }

    fn notify_subscribers(&self, mining_info: MiningInfo) -> Result<(), Error> {
        let miner_work = try!(miner::MinerWork::from_mining_info(mining_info));
        for sender in self.subscribers.lock().unwrap().deref() {
            try!(sender.send(miner_work.clone()))
        }
        Ok(())
    }

    pub fn submit_hash(&self, nonce: u64, account_id: u64) -> Result<String, Error> {
        let mut query_url = self.url.clone();
        match query_url.path_segments_mut() {
            Ok(mut path_segments) => {
                path_segments.pop_if_empty().push("burst");
            }
            Err(_) => return Err(Error::Url),
        };
        query_url.query_pairs_mut()
            .append_pair("requestType", "submitNonce")
            .append_pair("accountId", &account_id.to_string())
            .append_pair("nonce", &nonce.to_string());
        println!("{}", query_url);
        let mut response = String::new();
        let ref http_client = self.client.lock().unwrap();
        let mut res = try!(http_client.get(query_url).send());

        // assert_eq!(res.status, hyper::Ok);
        res.read_to_string(&mut response).unwrap();
        return Ok(response);
    }
}
