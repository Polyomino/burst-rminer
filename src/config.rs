extern crate rustc_serialize;

#[derive(RustcDecodable, RustcEncodable)]
pub struct MinerConfiguration {
    pub pool_url: Option<String>,
    pub plot_folders: Option<Vec<String>>,
    pub max_deadline: Option<u32>,
    pub plot_buffer_size: Option<u32>,
}
