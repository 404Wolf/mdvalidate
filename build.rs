fn main() {
    #[cfg(debug_assertions)]
    {
        println!("cargo:rustc-cfg=feature=\"invariant_violations\"");
    }
}
