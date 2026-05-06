pub mod compression;
pub mod crc;
pub mod vector;

pub fn hardware_available() -> bool {
    cfg!(feature = "gpu-cuda")
}
