extern crate libc;

use std::default::Default;

#[repr(C)]
pub struct sph_shabal_context {
    pub buf: [u8; 64],
    pub ptr: libc::size_t,
    pub a: [u32; 12],
    pub b: [u32; 16],
    pub c: [u32; 16],
    pub w_high: u32,
    pub w_low: u32,
}

impl Default for sph_shabal_context {
    fn default() -> sph_shabal_context {
        sph_shabal_context {
            buf: [0u8; 64],
            ptr: 0,
            a: [0u32; 12],
            b: [0u32; 16],
            c: [0u32; 16],
            w_high: 0,
            w_low: 0,
        }
    }
}

pub fn shabal256(input: &[u8]) -> [u8; 32] {
    let mut output: [u8; 32] = [0; 32];
    unsafe {
        let mut shabal_ctx: sph_shabal_context = Default::default();
        let shabal_ctx_ptr: *mut libc::c_void = &mut shabal_ctx as *mut _ as *mut libc::c_void;
        sph_shabal256_init(shabal_ctx_ptr);
        sph_shabal256(shabal_ctx_ptr,
                      &input[0] as *const _ as *const libc::c_void,
                      input.len());
        // println!("input:  {}", input.to_hex());
        sph_shabal256_close(shabal_ctx_ptr,
                            &mut output[0] as *mut _ as *mut libc::c_void);
        // println!("output: {}", output.to_hex());
    }
    return output;
}

extern "C" {
    pub fn sph_shabal256_init(cc: *mut libc::c_void);
    pub fn sph_shabal256(cc: *mut libc::c_void,
                         data: *const libc::c_void,
                         input_size: libc::size_t);
    pub fn sph_shabal256_close(cc: *mut libc::c_void, dst: *mut libc::c_void);
}