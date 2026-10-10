use std::collections::HashMap;
use std::ffi::CString;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::params::{ParamInfoBuffer, PluginParams};
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;

use crate::PluginRef;

/// A plugin found on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// The `.clap` file or bundle
    pub path: String,
}

/// One control of a plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginParam {
    pub id: u32,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

/// What the plugin thread is asked to do.
enum Request {
    Scan {
        extra: Vec<String>,
        reply: Sender<Vec<PluginInfo>>,
    },
    Params {
        plugin: PluginRef,
        reply: Sender<Result<Vec<PluginParam>, String>>,
    },
    Process {
        plugin: PluginRef,
        sample_rate: f64,
        audio: Vec<f32>,
        reply: Sender<Result<Vec<f32>, String>>,
    },
}

static THREAD: OnceLock<Mutex<Sender<Request>>> = OnceLock::new();
/// Plugins that live inside this program rather than on disk (tests use this).
static BUILTIN: OnceLock<Mutex<HashMap<String, PluginEntry>>> = OnceLock::new();

/// Make an in-process plugin available under a path-like name.
pub fn register_entry(path: &str, entry: PluginEntry) {
    BUILTIN
        .get_or_init(Default::default)
        .lock()
        .expect("registry")
        .insert(path.to_string(), entry);
}

fn sender() -> Sender<Request> {
    let m = THREAD.get_or_init(|| {
        let (tx, rx) = channel::<Request>();
        std::thread::Builder::new()
            .name("plugin-host".into())
            .spawn(move || {
                let mut loaded: HashMap<String, Loaded> = HashMap::new();
                for req in rx {
                    match req {
                        Request::Scan { extra, reply } => {
                            let _ = reply.send(scan_disk(&extra));
                        }
                        Request::Params { plugin, reply } => {
                            let _ = reply.send(params_of(&plugin));
                        }
                        Request::Process {
                            plugin,
                            sample_rate,
                            audio,
                            reply,
                        } => {
                            let key = format!("{}|{}", plugin.to_text(), sample_rate as u32);
                            let r = (|| {
                                if !loaded.contains_key(&key) {
                                    loaded.insert(key.clone(), Loaded::new(&plugin, sample_rate)?);
                                }
                                let l = loaded.get_mut(&key).expect("just inserted");
                                l.process(&audio)
                            })();
                            if r.is_err() {
                                loaded.remove(&key);
                            }
                            // few plugins are used at once; do not let abandoned ones pile up
                            if loaded.len() > 24 {
                                loaded.retain(|k, _| *k == key);
                            }
                            let _ = reply.send(r);
                        }
                    }
                }
            })
            .expect("spawn plugin thread");
        Mutex::new(tx)
    });
    m.lock().expect("plugin sender").clone()
}

/// Every plugin in the standard CLAP folders (and `extra` ones), by name.
pub fn scan(extra: &[String]) -> Vec<PluginInfo> {
    let (reply, rx) = channel();
    let _ = sender().send(Request::Scan {
        extra: extra.to_vec(),
        reply,
    });
    rx.recv().unwrap_or_default()
}

/// The controls a plugin offers.
pub fn params(plugin: &PluginRef) -> Result<Vec<PluginParam>, String> {
    let (reply, rx) = channel();
    sender()
        .send(Request::Params {
            plugin: plugin.clone(),
            reply,
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// Run interleaved stereo audio through a plugin. The plugin stays loaded between calls, so tails and
/// internal state carry on from one block to the next.
pub fn process(plugin: &PluginRef, sample_rate: f64, audio: &[f32]) -> Result<Vec<f32>, String> {
    let (reply, rx) = channel();
    sender()
        .send(Request::Process {
            plugin: plugin.clone(),
            sample_rate,
            audio: audio.to_vec(),
            reply,
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

fn entry_for(path: &str) -> Result<PluginEntry, String> {
    if let Some(e) = BUILTIN
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|m| m.get(path).cloned())
    {
        return Ok(e);
    }
    // SAFETY: loading a plugin runs its code; that is what the user asked for by choosing it
    unsafe { PluginEntry::load(path) }.map_err(|e| format!("cannot load {path}: {e}"))
}

fn scan_disk(extra: &[String]) -> Vec<PluginInfo> {
    let mut paths = clack_finder::standard_clap_paths();
    paths.extend(extra.iter().map(std::path::PathBuf::from));
    let mut out = Vec::new();
    for file in clack_finder::ClapFinder::new(paths) {
        let bundle = file.bundle_path().to_string_lossy().into_owned();
        let Ok(entry) = entry_for(&bundle) else {
            continue;
        };
        out.extend(describe(&entry, &bundle));
    }
    if let Some(m) = BUILTIN.get().and_then(|m| m.lock().ok()) {
        for (path, entry) in m.iter() {
            out.extend(describe(entry, path));
        }
    }
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

fn describe(entry: &PluginEntry, path: &str) -> Vec<PluginInfo> {
    let Some(factory) = entry.get_plugin_factory() else {
        return Vec::new();
    };
    factory
        .plugin_descriptors()
        .filter_map(|d| {
            let id = d.id()?.to_string_lossy().into_owned();
            Some(PluginInfo {
                name: d
                    .name()
                    .map_or(id.clone(), |n| n.to_string_lossy().into_owned()),
                vendor: d
                    .vendor()
                    .map_or(String::new(), |n| n.to_string_lossy().into_owned()),
                id,
                path: path.to_string(),
            })
        })
        .collect()
}

struct Host;

struct Shared;

impl SharedHandler<'_> for Shared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

struct MainThread;

impl MainThreadHandler<'_> for MainThread {}

impl HostHandlers for Host {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread;
    type AudioProcessor<'a> = ();
}

fn host_info() -> Result<HostInfo, String> {
    HostInfo::new("captainE", "captainE", "", env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())
}

fn make_instance(plugin: &PluginRef) -> Result<PluginInstance<Host>, String> {
    let entry = entry_for(&plugin.path)?;
    let id = CString::new(plugin.id.as_str()).map_err(|e| e.to_string())?;
    PluginInstance::<Host>::new(|_| Shared, |_| MainThread, &entry, &id, &host_info()?)
        .map_err(|e| format!("cannot start {}: {e}", plugin.id))
}

fn params_of(plugin: &PluginRef) -> Result<Vec<PluginParam>, String> {
    let mut instance = make_instance(plugin)?;
    let handle = instance.plugin_handle();
    let Some(ext) = handle.get_extension::<PluginParams>() else {
        return Ok(Vec::new());
    };
    let n = ext.count(&handle);
    let mut buf = ParamInfoBuffer::new();
    let mut out = Vec::new();
    for i in 0..n {
        if let Some(info) = ext.get_info(&handle, i, &mut buf) {
            out.push(PluginParam {
                id: info.id.get(),
                name: String::from_utf8_lossy(info.name).into_owned(),
                min: info.min_value,
                max: info.max_value,
                default: info.default_value,
            });
        }
    }
    Ok(out)
}

const BLOCK: usize = 2048;

struct Loaded {
    // the started processor must be dropped before the instance
    processor: Option<StartedPluginAudioProcessor<Host>>,
    _instance: PluginInstance<Host>,
    in_channels: usize,
    out_channels: usize,
    pending: Vec<(u32, f64)>,
    steady: u64,
}

impl Loaded {
    fn new(plugin: &PluginRef, sample_rate: f64) -> Result<Loaded, String> {
        let mut instance = make_instance(plugin)?;
        let handle = instance.plugin_handle();
        let channels = |is_input: bool| -> usize {
            handle
                .get_extension::<PluginAudioPorts>()
                .and_then(|ext| {
                    let mut buf = AudioPortInfoBuffer::new();
                    ext.get(&handle, 0, is_input, &mut buf)
                        .map(|i| i.channel_count as usize)
                })
                .unwrap_or(2)
                .clamp(1, 2)
        };
        let (in_channels, out_channels) = (channels(true), channels(false));
        let config = PluginAudioConfiguration {
            sample_rate,
            min_frames_count: 1,
            max_frames_count: BLOCK as u32,
        };
        let processor = instance
            .activate(|_, _| (), config)
            .map_err(|e| e.to_string())?
            .start_processing()
            .map_err(|e| e.to_string())?;
        Ok(Loaded {
            processor: Some(processor),
            _instance: instance,
            in_channels,
            out_channels,
            pending: plugin.params.clone(),
            steady: 0,
        })
    }

    fn process(&mut self, audio: &[f32]) -> Result<Vec<f32>, String> {
        let frames = audio.len() / 2;
        let mut out = vec![0.0f32; audio.len()];
        let mut at = 0;
        let processor = self.processor.as_mut().ok_or("plugin stopped")?;
        while at < frames {
            let n = (frames - at).min(BLOCK);
            let mut ins = vec![vec![0.0f32; n]; self.in_channels];
            for i in 0..n {
                let (l, r) = (audio[(at + i) * 2], audio[(at + i) * 2 + 1]);
                if self.in_channels == 1 {
                    ins[0][i] = (l + r) * 0.5;
                } else {
                    ins[0][i] = l;
                    ins[1][i] = r;
                }
            }
            let mut outs = vec![vec![0.0f32; n]; self.out_channels];
            let mut events = EventBuffer::with_capacity(self.pending.len().max(1));
            for (id, v) in self.pending.drain(..) {
                if let Some(cid) = ClapId::from_raw(id) {
                    events.push(&ParamValueEvent::new(0, cid, Pckn::match_all(), v));
                }
            }
            let mut in_ports = AudioPorts::with_capacity(self.in_channels, 1);
            let mut out_ports = AudioPorts::with_capacity(self.out_channels, 1);
            let input = in_ports.with_input_buffers([AudioPortBuffer {
                channels: AudioPortBufferType::f32_input_only(
                    ins.iter_mut().map(InputChannel::variable),
                ),
                latency: 0,
            }]);
            let mut output = out_ports.with_output_buffers([AudioPortBuffer {
                channels: AudioPortBufferType::f32_output_only(
                    outs.iter_mut().map(|b| b.as_mut_slice()),
                ),
                latency: 0,
            }]);
            processor
                .process(
                    &input,
                    &mut output,
                    &events.as_input(),
                    &mut OutputEvents::void(),
                    Some(self.steady),
                    None,
                )
                .map_err(|e| format!("the plugin failed: {e}"))?;
            self.steady += n as u64;
            for i in 0..n {
                let (l, r) = if self.out_channels == 1 {
                    (outs[0][i], outs[0][i])
                } else {
                    (outs[0][i], outs[1][i])
                };
                out[(at + i) * 2] = l;
                out[(at + i) * 2 + 1] = r;
            }
            at += n;
        }
        Ok(out)
    }
}

impl Drop for Loaded {
    fn drop(&mut self) {
        if let Some(p) = self.processor.take() {
            self._instance.deactivate(p.stop_processing());
        }
    }
}
