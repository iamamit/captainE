//! Keep only the sequence with this name in the project (on the current branch), so tools work on it.
//!     seq_keep <project.json> "<sequence name>"
use captaine_ops::{Author, Op};
use captaine_project::Session;
use std::path::Path;
fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let proj = Path::new(&a[1]);
    let mut s = Session::load(proj)?;
    let keep = a[2].clone();
    let all: Vec<(String, String)> = s
        .editor
        .project()
        .timelines
        .values()
        .map(|t| (t.id.to_string(), t.name.clone()))
        .collect();
    if !all.iter().any(|(_, n)| *n == keep) {
        return Err(format!("no sequence '{keep}': {all:?}"));
    }
    let ids: Vec<_> = s
        .editor
        .project()
        .timelines
        .values()
        .filter(|t| t.name != keep)
        .map(|t| t.id)
        .collect();
    for id in ids {
        s.apply_as(Author::Script("seq_keep".into()), Op::RemoveTimeline { id })?;
    }
    s.commit(Some(&format!("keep only {keep}")))?;
    s.save(proj)?;
    println!("kept {keep}; had {all:?}");
    Ok(())
}
