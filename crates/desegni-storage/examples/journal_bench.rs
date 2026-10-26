//! Times journal appends on a real recovered project: the first entry writes everything, the
//! rest mimic keystroke edits that must not rewrite the imported site files every time.
//!
//! cargo run --release -p desegni-storage --example journal_bench -- <journal path>

use desegni_storage::Journal;
use std::{env, fs, path::PathBuf, time::Instant};

const APPENDS: usize = 20;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = env::args().nth(1).ok_or("Pass a recovery journal path.")?;
    let mut project =
        Journal::recover(PathBuf::from(&source).as_path())?.ok_or("Empty journal.")?;
    // Keep the journal on the real disk, not on a tmpfs.
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/journal-bench");
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }
    let journal = Journal::new(&directory, project.id)?;

    let name = project.name.clone();
    let mut times = Vec::with_capacity(APPENDS);
    for index in 1..=APPENDS {
        project.revision += 1;
        project.name = format!("{name} {index}");
        let started = Instant::now();
        journal.append(&project)?;
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    let mut steady = times[1..].to_vec();
    steady.sort_by(f64::total_cmp);
    let median = steady[steady.len() / 2];
    println!("first append: {:.1} ms", times[0]);
    println!(
        "appends 2..={APPENDS}: median {median:.1} ms, min {:.1} ms, max {:.1} ms",
        steady[0],
        steady[steady.len() - 1]
    );

    let started = Instant::now();
    let recovered = Journal::recover(journal.path())?;
    println!(
        "recover: {:.1} ms (revision {:?})",
        started.elapsed().as_secs_f64() * 1000.0,
        recovered.map(|project| project.revision)
    );
    fs::remove_dir_all(&directory)?;
    Ok(())
}
