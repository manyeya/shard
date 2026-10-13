// Gives shard.exe its icon on Windows. Other platforms take theirs from the package around the
// binary (Shard.app, the .desktop file), so there is nothing to do there.
fn main() {
    println!("cargo:rerun-if-changed=assets/icons/shard.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/shard.ico");
        res.compile().expect("embed the Windows icon");
    }
}
