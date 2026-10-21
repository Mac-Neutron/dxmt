//! The facade's tests run against a winemetal.so outside Wine.
//!
//! `MTLD3D_WINEMETAL` names one beside stand-ins for the Wine libraries it links (the facade
//! test script builds that folder).

#![allow(dead_code)]

use mtld3d_mtl::{
    objc2::{Retained, runtime::ProtocolObject},
    objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice},
};

pub fn device() -> Retained<ProtocolObject<dyn MTLDevice>> {
    assert!(
        std::env::var_os("MTLD3D_WINEMETAL").is_some(),
        "MTLD3D_WINEMETAL names no winemetal.so: run dxmt/mtld3d-facade-test.sh"
    );
    MTLCreateSystemDefaultDevice().expect("a device from winemetal")
}
