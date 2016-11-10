extern crate rustc_serialize;

#[derive(RustcDecodable, RustcEncodable)]
pub struct MinerConfiguration {
    pub pool_url: Option<String>,
    pub plot_folders: Option<Vec<String>>,
    pub threads_per_folder: Option<u32>,
    pub max_deadline: Option<u32>,
}