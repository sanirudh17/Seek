fn main() {
    println!("cargo:rerun-if-changed=assets/seek-helper.rc");
    println!("cargo:rerun-if-changed=../../app/assets/seek.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/seek-helper.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compile optional helper version resource");
    }
}
