//! Builds the pinned libaom and libjxl (see scripts/fetch-deps.sh) as static
//! libraries, generates Rust bindings from their headers, and runs the
//! napi-rs setup.

use std::env;
use std::path::PathBuf;

fn main() {
    if env::var_os("CARGO_FEATURE_NAPI").is_some() {
        napi_build::setup();
    }
    build_aom();
    build_jxl();
}

fn vendor_dir(name: &str, env_var: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = env::var_os(env_var).map(PathBuf::from).unwrap_or_else(|| manifest_dir.join("vendor").join(name));
    println!("cargo:rerun-if-env-changed={env_var}");
    println!("cargo:rerun-if-changed={}", src.join(".version").display());
    if !src.join("CMakeLists.txt").exists() {
        panic!("{name} source not found at {}. Run scripts/fetch-deps.sh first.", src.display());
    }
    src
}

fn bindings(header: &str, include: &std::path::Path, prefix: &str, out: &str) {
    bindgen::Builder::default()
        .header_contents("wrap.h", header)
        .clang_arg(format!("-I{}", include.display()))
        .allowlist_function(format!("{prefix}.*"))
        .allowlist_type(format!("{prefix}.*"))
        .allowlist_var(format!("{}.*", prefix.to_uppercase()))
        .default_enum_style(bindgen::EnumVariation::Consts)
        .layout_tests(false)
        .generate()
        .unwrap_or_else(|e| panic!("generate {out}: {e}"))
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join(out))
        .unwrap();
}

fn build_jxl() {
    let src = vendor_dir("libjxl", "IMGCONV_JXL_SOURCE_DIR");
    let out = cmake::Config::new(&src)
        .profile("Release")
        .define("BUILD_TESTING", "OFF")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("JPEGXL_ENABLE_TOOLS", "OFF")
        .define("JPEGXL_ENABLE_DOXYGEN", "OFF")
        .define("JPEGXL_ENABLE_MANPAGES", "OFF")
        .define("JPEGXL_ENABLE_BENCHMARK", "OFF")
        .define("JPEGXL_ENABLE_EXAMPLES", "OFF")
        .define("JPEGXL_ENABLE_JNI", "OFF")
        .define("JPEGXL_ENABLE_SJPEG", "OFF")
        .define("JPEGXL_ENABLE_OPENEXR", "OFF")
        .define("JPEGXL_ENABLE_JPEGLI", "OFF")
        .define("JPEGXL_ENABLE_TCMALLOC", "OFF")
        .define("JPEGXL_BUNDLE_LIBPNG", "OFF")
        .define("JPEGXL_FORCE_SYSTEM_BROTLI", "OFF")
        .define("JPEGXL_FORCE_SYSTEM_HWY", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("CMAKE_INSTALL_LIBDIR", "lib")
        .build();
    println!("cargo:rustc-link-search=native={}", out.join("lib").display());
    for lib in ["jxl", "jxl_cms", "jxl_threads", "hwy", "brotlienc", "brotlidec", "brotlicommon"] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-lib=stdc++");
    bindings(
        "#include <jxl/encode.h>\n#include <jxl/decode.h>\n#include <jxl/thread_parallel_runner.h>\n",
        &out.join("include"),
        "Jxl",
        "jxl_bindings.rs",
    );
}

fn build_aom() {
    let src = vendor_dir("libaom", "IMGCONV_AOM_SOURCE_DIR");

    let target = env::var("TARGET").unwrap();
    let mut cfg = cmake::Config::new(&src);
    cfg.profile("Release")
        .define("ENABLE_DOCS", "0")
        .define("ENABLE_EXAMPLES", "0")
        .define("ENABLE_TESTDATA", "0")
        .define("ENABLE_TESTS", "0")
        .define("ENABLE_TOOLS", "0")
        .define("ENABLE_NASM", "1")
        .define("CONFIG_AV1_ENCODER", "1")
        .define("CONFIG_AV1_DECODER", "1")
        .define("CONFIG_PIC", "1")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("CMAKE_INSTALL_LIBDIR", "lib");
    if target.starts_with("aarch64") && env::var("HOST").unwrap() != target {
        cfg.define("CMAKE_SYSTEM_PROCESSOR", "aarch64")
            .define("CMAKE_SYSTEM_NAME", "Linux");
    }
    let out = cfg.build();

    println!("cargo:rustc-link-search=native={}", out.join("lib").display());
    println!("cargo:rustc-link-lib=static=aom");

    let include = out.join("include");
    let bindings = bindgen::Builder::default()
        .header_contents(
            "aom_wrap.h",
            "#include <aom/aom_encoder.h>\n#include <aom/aomcx.h>\n#include <aom/aom_decoder.h>\n#include <aom/aomdx.h>\n",
        )
        .clang_arg(format!("-I{}", include.display()))
        .allowlist_function("aom_.*")
        .allowlist_type("aom_.*|aome_.*|aomd_.*|av1_.*|aom_codec_control_type_.*")
        .allowlist_var("AOM_.*|AV1E_.*|AOME_.*|AV1_.*")
        .default_enum_style(bindgen::EnumVariation::Consts)
        .layout_tests(false)
        .generate()
        .expect("generate libaom bindings");
    bindings
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("aom_bindings.rs"))
        .unwrap();
}
