pub mod compute;

pub fn hardware_available() -> bool {
    cfg!(feature = "gpu-vulkan")
}
