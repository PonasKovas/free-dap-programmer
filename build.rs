fn main() {
    cc::Build::new()
        .file("dap/dap.c")
        .include("dap")
        // Pass ARM Cortex-M target options matching your MCU
        .flag("-mcpu=cortex-m4")
        .flag("-mthumb")
        .compile("free_dap");

    println!("cargo:rerun-if-changed=dap/");
}
