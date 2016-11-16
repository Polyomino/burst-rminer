use constants::*;
use regex::Regex;
use sph_shabal;
use std::path::PathBuf;
use std::fs;
use std::io;
use libc;

#[derive(Clone)]
pub struct Plot {
    pub path: PathBuf,
    pub account_id: u64,
    pub start_nonce: u64,
    pub nonce_count: u64,
    pub stagger_size: u64,
}

pub struct PlotFolder {
    pub path: PathBuf,
    pub plots: Vec<Plot>,
}

pub struct PlotFolders {
    pub folders: Vec<PlotFolder>,
}

pub fn get_plots(folder_paths: Vec<String>) -> PlotFolders {
    PlotFolders {
        folders: folder_paths.into_iter()
            .map(|folder_path| {
                let path_buf = PathBuf::from(folder_path);
                let plot_paths = match path_buf.is_dir() {
                        true => {
                            let combine_paths =
                                |file_path: Result<fs::DirEntry, io::Error>| -> PathBuf {
                                    path_buf.as_path()
                                        .join(file_path.unwrap().path())
                                };
                            Ok(path_buf.read_dir()
                                .unwrap()
                                .map(combine_paths)
                                .collect::<Vec<PathBuf>>())
                        }
                        false => Err(format!("plot folder '{:?}' is not a folder", path_buf)),
                    }
                    .unwrap();
                let plot_regex = Regex::new(r"^(\d+)_(\d+)_(\d+)_(\d+)$").unwrap();

                let plots = plot_paths.iter()
                    .map(|plot_path| {
                        let filename = plot_path.as_path().file_name().unwrap();

                        match plot_regex.captures(filename.to_str().unwrap()) {
                                Some(captures) => {
                                    Ok(Plot {
                                        path: plot_path.clone(),
                                        account_id: captures.at(1)
                                            .unwrap()
                                            .parse::<u64>()
                                            .unwrap(),
                                        start_nonce: captures.at(2)
                                            .unwrap()
                                            .parse::<u64>()
                                            .unwrap(),
                                        nonce_count: captures.at(3)
                                            .unwrap()
                                            .parse::<u64>()
                                            .unwrap(),
                                        stagger_size: captures.at(4)
                                            .unwrap()
                                            .parse::<u64>()
                                            .unwrap(),
                                    })
                                }
                                None => Err("invalid plot file name"),
                            }
                            .unwrap()
                    })
                    .collect();

                PlotFolder {
                    path: path_buf,
                    plots: plots,
                }
            })
            .collect(),
    }
}

#[allow(dead_code)]
fn generate_plot(input: [u64; 2]) -> [u8; PLOT_SIZE + 16] {
    let mut output: [u8; PLOT_SIZE + 16] = [0; PLOT_SIZE + 16];
    unsafe {
        libc::memcpy(&mut output[PLOT_SIZE] as *mut _ as *mut libc::c_void,
                     &input as *const _ as *const libc::c_void,
                     16 as libc::size_t);
    }
    let mut i = PLOT_SIZE;
    let mut shabal_ctx: sph_shabal::sph_shabal_context = Default::default();
    let shabal_ctx_ptr: *mut libc::c_void = &mut shabal_ctx as *mut _ as *mut libc::c_void;
    while i > 0 {
        let mut len: libc::size_t = PLOT_SIZE + 16 - i;
        if len > HASH_CAP {
            len = HASH_CAP;
        }
        unsafe {
            sph_shabal::sph_shabal256_init(shabal_ctx_ptr);
            sph_shabal::sph_shabal256(shabal_ctx_ptr,
                                      &output[i] as *const _ as *const libc::c_void,
                                      len);
            sph_shabal::sph_shabal256_close(shabal_ctx_ptr,
                                &mut output[i - HASH_SIZE] as *mut _ as *mut libc::c_void);
        }
        i -= HASH_SIZE;
    }

    let mut last_hash: [u8; 32] = [0; 32];
    unsafe {
        sph_shabal::sph_shabal256_init(shabal_ctx_ptr);
        sph_shabal::sph_shabal256(shabal_ctx_ptr,
                                  &output as *const _ as *const libc::c_void,
                                  16 + PLOT_SIZE);
        sph_shabal::sph_shabal256_close(shabal_ctx_ptr,
                                        &mut last_hash as *mut _ as *mut libc::c_void);
    }

    for i in 0..PLOT_SIZE {
        output[i] ^= last_hash[i % 32];
    }

    // println!("{:?}",output);
    return output;
}
