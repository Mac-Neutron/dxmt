const DELEGATE_FORWARD: &str = "src/metal/macdrv/delegate_forward.m";

fn main() {
    println!("cargo::rustc-check-cfg=cfg(perf_tracking)");
    println!("cargo:rerun-if-env-changed=MTLD3D_PERF");
    if std::env::var("MTLD3D_PERF").is_ok_and(|value| !value.is_empty() && value != "0") {
        println!("cargo:rustc-cfg=perf_tracking");
    }
    println!("cargo::rustc-check-cfg=cfg(mtld3d_crumb)");
    println!("cargo:rerun-if-env-changed=MTLD3D_CRUMB");
    if std::env::var("MTLD3D_CRUMB").is_ok_and(|v| !v.is_empty() && v != "0") {
        println!("cargo:rustc-cfg=mtld3d_crumb");
    }

    let target = std::env::var("TARGET").unwrap();
    // MacNeutron: `mtl_facade` when the Metal layer goes through winemetal (src/mtl.rs): the `winemetal` feature, and
    // every build off Apple targets.
    println!("cargo::rustc-check-cfg=cfg(mtl_facade)");
    let facade = std::env::var_os("CARGO_FEATURE_WINEMETAL").is_some() || !target.contains("apple");
    if facade {
        println!("cargo:rustc-cfg=mtl_facade");
    }
    if !target.contains("apple") {
        return;
    }

    println!("cargo:rustc-link-arg-cdylib=-Wl,-install_name,@rpath/mtld3d.so");

    // Every Rust frame of this crate aborts on unwind (`panic = "abort"`),
    // so a message send whose Objective-C exception has to be caught keeps
    // its `@try` and the send together in Objective-C.
    if facade {
        return;
    }
    println!("cargo:rerun-if-changed={DELEGATE_FORWARD}");
    cc::Build::new()
        .file(DELEGATE_FORWARD)
        .flag("-fobjc-exceptions")
        .flag("-fno-objc-arc")
        .warnings_into_errors(true)
        .compile("mtld3d_delegate_forward");
}
