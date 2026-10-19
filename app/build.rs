fn main() {
    println!("cargo:rerun-if-changed=assets/seek.rc");
    println!("cargo:rerun-if-changed=assets/seek.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("assets/seek.rc", ["seek"], embed_resource::NONE)
            .manifest_required()
            .expect("compile Seek application icon");
    }
    // GPUI's nested settings layout needs room in unoptimized MSVC builds.
    // Windows reserves this address space and commits stack pages on demand.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-bin=seek=/STACK:8388608");
    }
}
