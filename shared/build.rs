fn main() {
    println!("cargo:rerun-if-changed=../runtime/");

    cc::Build::new()
        .file("../runtime/w4on2.c")
        .flag("-O2") // required thanks to _FORTIFY_SOURCE error...
        .flag("-Wall")
        // .flag("-Wextra") doesn't work on MSVC lol
        // .flag("-Werror") doesn't work on MSVC lol
        .compile("w4on2_runtime");

    let bindings = bindgen::Builder::default()
        .header("../runtime/w4on2.h")
        .generate()
        .unwrap();
    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("w4on2_runtime_bindings.rs"))
        .expect("Couldn't write bindings!");
}
