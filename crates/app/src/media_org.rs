//! Organising the media pool: user bins, tags and notes, search across them, and proxy files.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use captaine_model::{Id, MediaMeta, MediaRef};
use captaine_ops::Op;

use crate::app::App;

/// Finished proxy files, sent back from the worker thread.
pub struct ProxyJobs {
    tx: Sender<(Id, Result<String, String>)>,
    rx: Receiver<(Id, Result<String, String>)>,
    /// media still being transcoded
    pub busy: Vec<Id>,
}

impl Default for ProxyJobs {
    fn default() -> Self {
        let (tx, rx) = channel();
        ProxyJobs {
            tx,
            rx,
            busy: Vec::new(),
        }
    }
}

/// Bin names with the number of items in each, by name.
pub fn bin_counts(
    meta: &BTreeMap<Id, MediaMeta>,
    media: &BTreeMap<Id, MediaRef>,
) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (id, m) in meta {
        if media.contains_key(id) && !m.bin.is_empty() {
            *counts.entry(m.bin.clone()).or_default() += 1;
        }
    }
    counts.into_iter().collect()
}

/// Does `m` (with its organising, if any) match what was typed in the search box?
/// Every word has to match the name, or the bin, a tag or the note.
pub fn matches_search(m: &MediaRef, meta: Option<&MediaMeta>, query: &str) -> bool {
    let name = m.name.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|w| name.contains(w) || meta.is_some_and(|x| x.matches(w)))
}

/// "tag one, tag two" -> the list.
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for t in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !v.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            v.push(t.to_string());
        }
    }
    v
}

impl App {
    pub fn media_meta(&self, id: Id) -> MediaMeta {
        self.project().meta.get(&id).cloned().unwrap_or_default()
    }

    pub fn set_media_meta(&mut self, id: Id, meta: MediaMeta) {
        if self.media_meta(id) != meta {
            self.do_op(Op::SetMediaMeta {
                media: id,
                meta: Some(meta),
            });
        }
    }

    pub fn move_to_bin(&mut self, id: Id, bin: &str) {
        let mut m = self.media_meta(id);
        m.bin = bin.trim().to_string();
        self.set_media_meta(id, m);
    }

    fn proxy_dir(&self) -> PathBuf {
        match &self.path {
            Some(p) => {
                let mut name = p.file_name().map(|n| n.to_os_string()).unwrap_or_default();
                name.push(".cache");
                p.with_file_name(name)
            }
            None => std::env::temp_dir().join("captaine-proxies"),
        }
    }

    /// Make proxy files for these videos in the background (those that already have one are skipped).
    pub fn make_proxies(&mut self, ids: &[Id], height: u32) {
        let dir = self.proxy_dir();
        let todo: Vec<MediaRef> = ids
            .iter()
            .filter_map(|id| self.project().media.get(id))
            .filter(|m| {
                m.has_video
                    && m.nested_timeline().is_none()
                    && m.proxy.is_none()
                    && !self.proxies.busy.contains(&m.id)
            })
            .cloned()
            .collect();
        if todo.is_empty() {
            self.toast("Nothing to do: every video already has a proxy", false);
            return;
        }
        self.proxies.busy.extend(todo.iter().map(|m| m.id));
        let (tx, ctx) = (self.proxies.tx.clone(), self.ctx.clone());
        self.toast(format!("Making {} proxy file(s)…", todo.len()), false);
        std::thread::Builder::new()
            .name("proxies".into())
            .spawn(move || {
                for m in todo {
                    let dst = captaine_media::cache_path(&dir, m.id, "proxy.mp4");
                    let r = captaine_media::make_proxy(std::path::Path::new(&m.path), &dst, height)
                        .map(|_| dst.to_string_lossy().into_owned())
                        .map_err(|e| format!("{}: {e}", m.name));
                    let _ = tx.send((m.id, r));
                    ctx.request_repaint();
                }
            })
            .expect("spawn proxy thread");
    }

    /// Attach the proxy files that have been finished since the last call.
    pub fn poll_proxies(&mut self) {
        let done: Vec<_> = self.proxies.rx.try_iter().collect();
        for (id, r) in done {
            self.proxies.busy.retain(|b| *b != id);
            match r {
                Ok(path) => {
                    if self.project().media.contains_key(&id) {
                        self.do_op(Op::SetProxy {
                            media: id,
                            proxy: Some(path),
                        });
                        let left = self.proxies.busy.len();
                        if left == 0 {
                            self.toast("Proxies are ready", false);
                        }
                    }
                }
                Err(e) => self.toast(e, true),
            }
        }
    }
}
