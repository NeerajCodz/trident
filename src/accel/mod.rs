pub mod cpu;
pub mod gpu;
pub mod traits;

pub use cpu::CpuAccelerator;
pub use gpu::GpuAccelerator;
pub use traits::Accelerator;
