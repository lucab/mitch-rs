extern crate prost_build;

use std::{env, fs, path};

fn main() {
    if !cfg!(feature = "protogen") {
        return;
    }

    let protos = vec!["protomitch"];

    prost_build::compile_protos(&["proto3/protomitch.proto"], &["proto3/"]).unwrap();

    let target_dir = path::PathBuf::from("./src/");
    let out_dir = path::PathBuf::from(env::var("OUT_DIR").unwrap());

    for p in protos {
        let buildname = format!("{}.rs", p);
        let mut buildfile = out_dir.clone();
        buildfile.push(&buildname);

        let targetname = format!("{}_pb.rs", p);
        let mut targetfile = target_dir.clone();
        targetfile.push(&targetname);

        fs::rename(&buildfile, &targetfile).unwrap();
    }
}
