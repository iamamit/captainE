//! Hosting CLAP audio plugins (the open plugin standard): finding them on disk, listing their controls,
//! and running audio through them. Plugin code is not thread-safe, so one thread owns every loaded plugin
//! and the rest of the program talks to it by message.

mod host;

pub use host::{params, process, register_entry, scan, PluginInfo, PluginParam};

/// A plugin as an effect remembers it: which one, where it lives and the values of its controls.
///
/// The text form is `clap:<plugin id>@<path>?<param id>=<value>&...`.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginRef {
    pub id: String,
    pub path: String,
    pub params: Vec<(u32, f64)>,
}

impl PluginRef {
    pub fn to_text(&self) -> String {
        let mut s = format!("clap:{}@{}", self.id, self.path);
        for (i, (id, v)) in self.params.iter().enumerate() {
            s.push(if i == 0 { '?' } else { '&' });
            s.push_str(&format!("{id}={v}"));
        }
        s
    }

    pub fn parse(text: &str) -> Option<PluginRef> {
        let rest = text.strip_prefix("clap:")?;
        let (id, rest) = rest.split_once('@')?;
        let (path, query) = match rest.rsplit_once('?') {
            // a path may contain '?' in theory; only treat it as a query when it holds name=value pairs
            Some((p, q))
                if q.split('&').all(|kv| {
                    kv.split_once('=')
                        .is_some_and(|(k, v)| k.parse::<u32>().is_ok() && v.parse::<f64>().is_ok())
                }) =>
            {
                (p, q)
            }
            _ => (rest, ""),
        };
        if id.is_empty() || path.is_empty() {
            return None;
        }
        let params = query
            .split('&')
            .filter(|kv| !kv.is_empty())
            .filter_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                Some((k.parse().ok()?, v.parse().ok()?))
            })
            .collect();
        Some(PluginRef {
            id: id.into(),
            path: path.into(),
            params,
        })
    }

    /// The value of control `id`, if it has been set.
    pub fn value(&self, id: u32) -> Option<f64> {
        self.params.iter().find(|(p, _)| *p == id).map(|(_, v)| *v)
    }

    pub fn with_value(mut self, id: u32, v: f64) -> PluginRef {
        match self.params.iter_mut().find(|(p, _)| *p == id) {
            Some(slot) => slot.1 = v,
            None => self.params.push((id, v)),
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_form_round_trips() {
        let r = PluginRef {
            id: "org.example.eq".into(),
            path: "/Library/Audio/Plug-Ins/CLAP/Eq.clap".into(),
            params: vec![(1, 0.5), (7, -3.25)],
        };
        assert_eq!(PluginRef::parse(&r.to_text()), Some(r.clone()));
        let bare = PluginRef {
            params: vec![],
            ..r.clone()
        };
        assert_eq!(PluginRef::parse(&bare.to_text()), Some(bare));
        assert_eq!(r.value(7), Some(-3.25));
        assert_eq!(r.clone().with_value(7, 1.0).value(7), Some(1.0));
        assert_eq!(r.with_value(9, 2.0).params.len(), 3);
        for bad in ["", "clap:", "clap:x", "clap:@/p", "vst:x@/p", "clap:id@"] {
            assert_eq!(PluginRef::parse(bad), None, "{bad:?}");
        }
    }
}
