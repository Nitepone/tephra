// The Lua API reference is embedded with include_dir!, which cargo can't track by itself.
fn main() {
    println!("cargo:rerun-if-changed=web/luadoc");
}
