use std::{env,
          fs::File,
          io::{BufReader, Read},
          path::{PathBuf, Path}};
use bindgen::{Bindings, BindgenError};

// SUNDIALS has a few non-negative constants that need to be parsed as an i32.
// This is an attempt at doing so generally.
#[derive(Debug)]
struct ParseSignedConstants;

impl bindgen::callbacks::ParseCallbacks for ParseSignedConstants {
    fn int_macro(&self, name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        let prefix: String = name.chars().take_while(|c| *c != '_').collect();
        match prefix.as_ref() {
            "CV" | "IDA" | "KIN" | "SUN" => Some(bindgen::callbacks::IntKind::Int),
            _ => None,
        }
    }
}

/// Build the Sundials code vendor with sundials-sys.
fn build_vendor_sundials() -> (Option<String>, Option<String>, &'static str) {
    macro_rules! feature {
        ($s:tt) => {
            if cfg!(feature = $s) {
                "ON"
            } else {
                "OFF"
            }
        };
    }

    let static_libraries = feature!("static_libraries");
    let (shared_libraries, library_type) = match static_libraries {
        "ON" => ("OFF", "static"),
        "OFF" => ("ON", "dylib"),
        _ => unreachable!(),
    };

    let dst = cmake::Config::new("vendor")
        .define("CMAKE_INSTALL_LIBDIR", "lib")
        .define("BUILD_STATIC_LIBS", static_libraries)
        .define("BUILD_SHARED_LIBS", shared_libraries)
        .define("BUILD_TESTING", "OFF")
        .define("EXAMPLES_INSTALL", "OFF")
        .define("EXAMPLES_ENABLE_C", "OFF")
        .define("BUILD_ARKODE", feature!("arkode"))
        .define("BUILD_CVODE", feature!("cvode"))
        .define("BUILD_CVODES", feature!("cvodes"))
        .define("BUILD_IDA", feature!("ida"))
        .define("BUILD_IDAS", feature!("idas"))
        .define("BUILD_KINSOL", feature!("kinsol"))
        .define("OPENMP_ENABLE", feature!("nvecopenmp"))
        .define("PTHREAD_ENABLE", feature!("nvecpthreads"))
        .build();
    let dst_disp = dst.display();
    let lib_loc = Some(format!("{}/lib", dst_disp));
    let inc_dir = Some(format!("{}/include", dst_disp));
    (lib_loc, inc_dir, library_type)
}

fn generate_bindings(inc_dir: &Option<String>)
                     -> Result<Bindings, BindgenError> {
    macro_rules! define {
        ($a:tt, $b:tt) => {
            format!(
                "-DUSE_{}={}",
                stringify!($b),
                if cfg!(feature = $a) { 1 } else { 0 }
            )
        };
    }

    bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(match inc_dir {
            Some(dir) => format!("-I{}", dir),
            None => "".to_owned(),
        })
        .clang_args(&[
            define!("arkode", ARKODE),
            define!("cvode", CVODE),
            define!("cvodes", CVODES),
            define!("ida", IDA),
            define!("idas", IDAS),
            define!("kinsol", KINSOL),
            define!("nvecopenmp", OPENMP),
            define!("nvecpthreads", PTHREADS),
        ])
        .parse_callbacks(Box::new(ParseSignedConstants))
        .generate()
}

fn sundials_major_version(bindings: impl AsRef<Path>) -> Option<u32> {
    let b = File::open(bindings).expect("Couldn't read file bindings.rs!");
    let mut b = BufReader::new(b).bytes();
    'version:
    while b.find(|c| c.as_ref().is_ok_and(|&c| c == b'S')).is_some() {
        for c0 in "UNDIALS_VERSION_MAJOR".bytes() {
            match b.next() {
                Some(Ok(c)) => {
                    if c != c0 {
                        continue 'version
                    }
                }
                Some(Err(_)) | None => return None
            }
        }
        // Match " : u32 = 6"
        if b.find(|c| c.as_ref().is_ok_and(|&c| c == b'=')).is_some() {
            let is_not_digit = |c: &u8| !c.is_ascii_digit();
            let b = b.skip_while(|c| c.as_ref().is_ok_and(is_not_digit));
            let v: Vec<_> =
                b.map_while(|c| c.ok().filter(|c| c.is_ascii_digit()))
                .collect();
            match String::from_utf8(v) {
                Ok(v) => return v.parse().ok(),
                Err(_) => return None
            }
        }
        return None
    }
    None
}

fn main() {
    // First, we build the SUNDIALS library, with requested modules with CMake

    let mut lib_loc;
    let mut inc_dir;
    let mut library_type = "dylib";
    if cfg!(any(feature = "build_libraries", target_family = "wasm")) {
        (lib_loc, inc_dir, library_type) = build_vendor_sundials();
    } else {
        lib_loc = env::var("SUNDIALS_LIBRARY_DIR").ok();
        inc_dir = env::var("SUNDIALS_INCLUDE_DIR").ok();
    }

    if lib_loc.is_none() && inc_dir.is_none() {
        #[cfg(target_family = "windows")] {
            let vcpkg = vcpkg::Config::new()
                .emit_includes(true)
                .find_package("sundials");
            if vcpkg.is_err() {
                (lib_loc, inc_dir, library_type) = build_vendor_sundials();
            }
        }
    }

    // Second, we use bindgen to generate the Rust types

    let bindings_rs = PathBuf::from(env::var("OUT_DIR").unwrap())
        .join("bindings.rs");
    let mut build_vendor = true;
    if let Ok(bindings) = generate_bindings(&inc_dir) {
        bindings.write_to_file(&bindings_rs)
            .expect("Couldn't write file bindings.rs!");
        if let Some(v) = sundials_major_version(&bindings_rs) {
            if v >= 6 {
                build_vendor = false
            } else {
                println!("cargo:warning=System sundials version = \
                          {} < 6, will use the vendor version", v);
            }
        }
    }
    if build_vendor {
        (lib_loc, inc_dir, library_type) = build_vendor_sundials();
        generate_bindings(&inc_dir)
            .expect("Unable to generate bindings of the vendor sundials!")
            .write_to_file(&bindings_rs)
            .expect("Couldn't write file bindings.rs!");
    }

    // Third, we let Cargo know about the library files

    if let Some(loc) = lib_loc {
        println!("cargo:rustc-link-search=native={}", loc)
    }
    for lib_name in &[
        "nvecserial",
        "sunlinsolband",
        "sunlinsoldense",
        "sunlinsolpcg",
        "sunlinsolspbcgs",
        "sunlinsolspfgmr",
        "sunlinsolspgmr",
        "sunlinsolsptfqmr",
        "sunmatrixband",
        "sunmatrixdense",
        "sunmatrixsparse",
        "sunnonlinsolfixedpoint",
        "sunnonlinsolnewton",
    ] {
        println!(
            "cargo:rustc-link-lib={}=sundials_{}",
            library_type, lib_name
        );
    }

    macro_rules! link {
        ($($s:tt),*) => {
            $(if cfg!(feature = $s) {
                println!("cargo:rustc-link-lib={}=sundials_{}", library_type, $s);
            })*
        }
    }

    link! {"arkode", "cvode", "cvodes", "ida", "idas", "kinsol",
           "nvecopenmp", "nvecpthreads"}

    // And that's all.
}
