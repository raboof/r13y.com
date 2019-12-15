use chrono::Utc;

use crate::{
    cas::ContentAddressedStorage,
    derivation::Derivation,
    diffoscope::Diffoscope,
    eval::{eval, JobInstantiation},
    messages::{BuildRequest, BuildStatus},
};

use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

pub fn report(instruction: BuildRequest) {
    let job = match instruction {
        BuildRequest::V1(ref req) => req.clone(),
    };

    let links = [
        ("x86_64-linux.iso", "https://github.com/NixOS/nixpkgs/pull/74174"),
        ("opensc", "https://github.com/OpenSC/OpenSC/pull/1839"),
        ("udisks", "https://github.com/storaged-project/udisks/issues/715"),
        ("gnupg", "https://github.com/NixOS/nixpkgs/issues/75687"),
    ];

    let JobInstantiation {
        to_build, results, ..
    } = eval(instruction.clone());

    let tmpdir = PathBuf::from("./tmp/");
    let report_dir = PathBuf::from("./report/");
    fs::create_dir_all(&report_dir).unwrap();
    let diff_dir = PathBuf::from("./report/diff");
    fs::create_dir_all(&diff_dir).unwrap();
    let mut html = File::create(report_dir.join("index.html")).unwrap();

    let read_cas = ContentAddressedStorage::new(tmpdir.clone());
    let write_cas = ContentAddressedStorage::new(report_dir.clone().join("cas"));
    let diffoscope = Diffoscope::new(write_cas.clone());
    let mut total = 0;
    let mut reproducible = 0;
    let mut unreproducible_list: Vec<String> = vec![];
    let mut unchecked = 0;
    let mut first_failed: Vec<String> = vec![];

    for response in results.into_iter().filter(|response| {
        (match response.request {
            BuildRequest::V1(ref req) => req.nixpkgs_revision == job.nixpkgs_revision,
        }) && to_build.contains(&PathBuf::from(&response.drv))
    }) {
        total += 1;
        match response.status {
            BuildStatus::Reproducible => {
                reproducible += 1;
            }
            BuildStatus::FirstFailed => {
                first_failed.push(response.drv);
            }
            BuildStatus::SecondFailed => {
                unchecked += 1;
            }
            BuildStatus::Unreproducible(hashes) => {
                let parsed_drv = Derivation::parse(&Path::new(&response.drv)).unwrap();

                unreproducible_list.push(format!("<li><code>{}</code><ul>", response.drv));
                for (keyword, link) in links.iter() {
                    if response.drv.contains(keyword) {
                        unreproducible_list.push(format!("<li><a href=\"{}\">more info...</a></li>", link));
                    }
                }
                for (output, (hash_a, hash_b)) in hashes.iter() {
                    if let Some(output_path) = parsed_drv.outputs().get(output) {
                        let dest_name = format!("{}-{}.html", hash_a, hash_b);
                        let dest = diff_dir.join(&dest_name);

                        if dest.exists() {
                            // ok
                        } else {
                            println!(
                                "Diffing {}'s {}: {} vs {}",
                                response.drv, output, hash_a, hash_b
                            );

                            let cas_a = read_cas.str_to_id(hash_a).unwrap();
                            let cas_b = read_cas.str_to_id(hash_b).unwrap();
                            let savedto = diffoscope
                                .nars(
                                    &output_path.file_name().unwrap().to_string_lossy(),
                                    &cas_a.as_path_buf(),
                                    &cas_b.as_path_buf(),
                                )
                                .unwrap();
                            println!("saved to: {}", savedto.display());
                            fs::copy(savedto, dest).unwrap();
                        }
                        unreproducible_list.push(format!(
                            "<li><a href=\"./diff/{}\">(diffoscope)</a> {}</li>",
                            dest_name, output
                        ));
                    } else {
                        println!("Diffing {} but no output named {}", response.drv, output);
                        // <li><a href="./diff/59nzffg69nprgg2zp8b36rqwha8vxzjk-perl-5.28.1.drv.html">(diffoscope)</a> <a href="./nix/store/59nzffg69nprgg2zp8b36rqwha8vxzjk-perl-5.28.1.drv">(drv)</a> <code>/nix/store/59nzffg69nprgg2zp8b36rqwha8vxzjk-perl-5.28.1.drv</code></li>
                    }
                }
                unreproducible_list.push("</ul></li>".to_string());

                println!("{:#?}", hashes);
            }
        }
    }

    if !first_failed.is_empty() {
        panic!("{} are unchecked:\n{:#?}", first_failed.len(), first_failed);
    }

    html.write_all(
        format!(
            include_str!("./template.html"),
            reproduced = reproducible,
            unchecked = unchecked,
            total = total,
            percent = format!("{:.*}%", 2, 100.0 * (reproducible as f64 / total as f64)),
            revision = job.nixpkgs_revision,
            now = Utc::now().to_string(),
            unreproduced_list = unreproducible_list.join("\n")
        )
        .as_bytes(),
    )
    .unwrap();
}
