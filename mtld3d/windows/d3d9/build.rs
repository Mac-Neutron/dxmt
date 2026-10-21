fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-cdylib-link-arg=/DEF:{manifest_dir}/d3d9.def");

    // The x86_64 DLL's linker map, `deps/d3d9.map` beside the linked image,
    // with every function's address, size and name. The Makefile's
    // `MEM_ROUTINE_GATE` reads it to find the in-image memory routines
    // (`src/guest_mem.rs`), which have no symbol table to be found by.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64") {
        println!("cargo:rustc-cdylib-link-arg=/lldmap");
    }

    // Diagnostic-only: `MTLD3D_CRUMB=1` enables the mmap breadcrumb in
    // `crate::crumb`. Routed through a build-script cfg instead of a
    // `RUSTFLAGS --cfg` flag so it composes with the xwin-link-path
    // rustflags set by `.cargo/config.toml` (env RUSTFLAGS *replaces*
    // those, it does not append).
    println!("cargo::rustc-check-cfg=cfg(mtld3d_crumb)");
    println!("cargo:rerun-if-env-changed=MTLD3D_CRUMB");
    if std::env::var("MTLD3D_CRUMB").is_ok_and(|v| !v.is_empty() && v != "0") {
        println!("cargo:rustc-cfg=mtld3d_crumb");
    }

    // MacNeutron: the x86 DLLs linked as static libraries on llvm-mingw's CRT, as
    // the ARM64X halves are (the VM guest's, `MTLD3D_MINGW_CRT`), take the CRT
    // pieces `src/arm64_crt.rs` supplies.
    println!("cargo::rustc-check-cfg=cfg(mingw_crt)");
    println!("cargo:rerun-if-env-changed=MTLD3D_MINGW_CRT");
    if std::env::var_os("MTLD3D_MINGW_CRT").is_some() {
        println!("cargo:rustc-cfg=mingw_crt");
    }

    println!("cargo::rustc-check-cfg=cfg(perf_tracking)");
    println!("cargo:rerun-if-env-changed=MTLD3D_PERF");
    if std::env::var("MTLD3D_PERF").is_ok_and(|v| !v.is_empty() && v != "0") {
        println!("cargo:rustc-cfg=perf_tracking");
    }
}
