use trident::io::{HardwareBackend, HardwareIoProfile, HardwareOperation};

#[test]
fn hardware_io_profile_declares_portable_cpu_fallbacks() {
    let profile = HardwareIoProfile::default();

    assert!(profile.supports(HardwareBackend::BufferedIo, HardwareOperation::Read));
    assert!(profile.has_cpu_fallback(HardwareOperation::AnnScoring));
    assert!(!profile.supports(HardwareBackend::Gpu, HardwareOperation::AnnScoring));
}
