//! Device reports shared by the settings screen and its simulated drivers.
//! Latency is reported in frames, at the driver's rate, never inferred from a requested buffer.
use std::collections::BTreeMap;

use crate::record::AudioInput;

pub const BUFFERS: [u32; 6] = [32, 64, 128, 256, 512, 1024];

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Latency {
    pub rate: u32,
    pub input: u32,
    pub output: u32,
    pub conversion: u32,
}

impl Latency {
    pub fn parts_ms(self) -> [f64; 3] {
        [self.input, self.output, self.conversion]
            .map(|n| n as f64 * 1000.0 / self.rate.max(1) as f64)
    }

    pub fn total_ms(self) -> f64 {
        self.parts_ms().iter().sum()
    }
}

/// A control-thread adapter. A real adapter must obtain reports from the opened stream.
pub trait Backend: Send {
    fn can_measure(&self) -> bool {
        true
    }
    fn devices(&self) -> Vec<Device>;
    fn configure(
        &mut self,
        driver: &str,
        device: &str,
        buffer: u32,
    ) -> Result<Option<Latency>, String>;
    fn levels(&mut self) -> Vec<f32>;
}

/// An input supplied through the same AudioInput interface as recording. No hardware is opened.
pub struct Simulated {
    pub devices: Vec<Device>,
    pub reports: BTreeMap<u32, Latency>,
    pub input: Box<dyn AudioInput>,
    selected: usize,
}

impl Simulated {
    pub fn new(
        devices: Vec<Device>,
        reports: BTreeMap<u32, Latency>,
        input: Box<dyn AudioInput>,
    ) -> Self {
        Self {
            devices,
            reports,
            input,
            selected: 0,
        }
    }
}

impl Backend for Simulated {
    fn devices(&self) -> Vec<Device> {
        self.devices.clone()
    }
    fn configure(&mut self, _: &str, device: &str, buffer: u32) -> Result<Option<Latency>, String> {
        self.selected = self
            .devices
            .iter()
            .position(|d| d.name == device)
            .ok_or_else(|| "オーディオデバイスが見つかりません".to_string())?;
        Ok(self.reports.get(&buffer).copied())
    }
    fn levels(&mut self) -> Vec<f32> {
        let Some(device) = self.devices.get(self.selected) else {
            return Vec::new();
        };
        let channels = self.input.channels();
        let mut samples = vec![0.0; channels * 256];
        self.input.read_channels(&mut samples, 256);
        samples
            .as_chunks::<256>()
            .0
            .iter()
            .take(device.inputs.len())
            .map(|s| {
                s.iter()
                    .filter(|v| v.is_finite())
                    .fold(0.0f32, |p, s| p.max(s.abs()))
            })
            .collect()
    }
}

/// Enumeration does not open a stream. Until an adapter reports latency the UI shows no number.
#[derive(Default)]
pub struct Unopened {
    pub devices: Vec<Device>,
}

impl Unopened {
    pub fn enumerate() -> Self {
        Self {
            devices: choices(&inventory())
                .into_iter()
                .map(|c| c.device)
                .collect(),
        }
    }
}

struct Endpoint {
    id: String,
    group: String,
    name: String,
    inputs: u16,
    outputs: u16,
    /// The system's default render endpoint.
    default: bool,
    /// The system's default capture endpoint.
    default_input: bool,
}
struct Choice {
    device: Device,
    input: Option<String>,
    output: Option<String>,
}

fn inventory() -> Vec<Endpoint> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.id().ok());
    let default_input = host.default_input_device().and_then(|d| d.id().ok());
    let Ok(list) = host.devices() else {
        return Vec::new();
    };
    let mut endpoints: Vec<_> = list
        .filter_map(|d| {
            let description = d.description().ok()?;
            let id = d.id().ok()?;
            Some(Endpoint {
                default: Some(&id) == default.as_ref(),
                default_input: Some(&id) == default_input.as_ref(),
                id: id.to_string(),
                group: device_label(&description),
                name: description.name().into(),
                inputs: d.default_input_config().map_or(0, |c| c.channels()),
                outputs: d.default_output_config().map_or(0, |c| c.channels()),
            })
        })
        .collect();
    endpoints.sort_by(|a, b| a.id.cmp(&b.id));
    endpoints
}

/// Capture/render endpoints are paired only inside their interface. Keep all alternatives.
fn choices(endpoints: &[Endpoint]) -> Vec<Choice> {
    let mut groups: BTreeMap<&str, Vec<&Endpoint>> = BTreeMap::new();
    for e in endpoints {
        groups.entry(&e.group).or_default().push(e);
    }
    let mut choices = Vec::new();
    for (group, list) in groups {
        let mut inputs: Vec<_> = list
            .iter()
            .copied()
            .filter(|e| e.inputs > 0)
            .map(Some)
            .collect();
        let mut outputs: Vec<_> = list
            .iter()
            .copied()
            .filter(|e| e.outputs > 0)
            .map(Some)
            .collect();
        if inputs.is_empty() {
            inputs.push(None);
        }
        if outputs.is_empty() {
            outputs.push(None);
        }
        for input in &inputs {
            for output in &outputs {
                let mut name = group.to_string();
                if inputs.len() > 1 || outputs.len() > 1 {
                    let details: Vec<_> = [*output, *input]
                        .into_iter()
                        .flatten()
                        .map(|e| e.name.as_str())
                        .collect();
                    name = format!("{group} — {}", details.join(" / "));
                }
                let base = name.clone();
                let mut n = 2;
                while choices.iter().any(|c: &Choice| c.device.name == name) {
                    name = format!("{base} ({n})");
                    n += 1;
                }
                choices.push(Choice {
                    device: Device {
                        name,
                        inputs: (1..=input.map_or(0, |e| e.inputs))
                            .map(|i| format!("In {i}"))
                            .collect(),
                        outputs: (1..=output.map_or(0, |e| e.outputs))
                            .map(|i| format!("Out {i}"))
                            .collect(),
                    },
                    input: input.map(|e| e.id.clone()),
                    output: output.map(|e| e.id.clone()),
                });
            }
        }
    }
    // The first is what a person who has chosen nothing gets: the system's default output, and
    // with it the system's default input, and failing that a real input before the monitor
    // mix a driver offers as a capture endpoint ("Loopback"), which carries no one's voice.
    let is = |id: &Option<String>, test: &dyn Fn(&Endpoint) -> bool| {
        endpoints
            .iter()
            .any(|e| test(e) && id.as_deref() == Some(&e.id))
    };
    choices.sort_by_key(|c| {
        (
            !is(&c.output, &|e| e.default),
            !is(&c.input, &|e| e.default_input),
            is(&c.input, &|e| is_monitor_mix(&e.name)),
        )
    });
    choices
}

/// A capture endpoint that is the output heard again, not a microphone or a line in.
fn is_monitor_mix(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("loopback") || name.contains("stereo mix") || name.contains("ステレオ ミキサー")
}

fn device_label(description: &cpal::DeviceDescription) -> String {
    // WASAPI exposes capture and render endpoints separately. Its interface name identifies
    // the adapter shown by the single device selector.
    if cfg!(windows) {
        description.driver().unwrap_or(description.name()).into()
    } else {
        description.name().into()
    }
}

/// Resolve the selected adapter to its capture or render endpoint without opening a stream.
pub fn endpoint(name: &str, input: bool) -> Result<cpal::Device, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if name.is_empty() {
        return if input {
            host.default_input_device()
        } else {
            host.default_output_device()
        }
        .ok_or_else(|| "オーディオデバイスが見つかりません".into());
    }
    let choice = choices(&inventory())
        .into_iter()
        .find(|c| c.device.name == name)
        .ok_or_else(|| format!("オーディオデバイスが見つかりません: {name}"))?;
    let id = if input { choice.input } else { choice.output };
    host.devices()
        .map_err(|e| e.to_string())?
        .find(|d| d.id().is_ok_and(|v| Some(v.to_string()) == id))
        .ok_or_else(|| {
            format!(
                "{name} の{}が見つかりません",
                if input { "入力" } else { "出力" }
            )
        })
}

impl Backend for Unopened {
    fn can_measure(&self) -> bool {
        false
    }
    fn devices(&self) -> Vec<Device> {
        self.devices.clone()
    }
    fn configure(&mut self, _: &str, _: &str, _: u32) -> Result<Option<Latency>, String> {
        Ok(None)
    }
    fn levels(&mut self) -> Vec<f32> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiple_render_endpoints_remain_distinct_and_the_default_is_first() {
        let endpoints = vec![
            Endpoint {
                id: "a".into(),
                group: "Interface".into(),
                name: "Speakers".into(),
                inputs: 0,
                outputs: 2,
                default: false,
                default_input: false,
            },
            Endpoint {
                id: "b".into(),
                group: "Interface".into(),
                name: "Headphones".into(),
                inputs: 0,
                outputs: 2,
                default: true,
                default_input: false,
            },
            Endpoint {
                id: "c".into(),
                group: "Interface".into(),
                name: "Mic".into(),
                inputs: 2,
                outputs: 0,
                default: false,
                default_input: false,
            },
        ];
        let list = choices(&endpoints);
        assert_eq!(list.len(), 2);
        assert_ne!(list[0].device.name, list[1].device.name);
        assert_eq!(list[0].output.as_deref(), Some("b"));
        assert_eq!(list[1].output.as_deref(), Some("a"));
        assert!(list.iter().all(|c| c.input.as_deref() == Some("c")));
    }
    fn endpoint(id: &str, name: &str, inputs: u16, outputs: u16) -> Endpoint {
        Endpoint {
            id: id.into(),
            group: "Interface".into(),
            name: name.into(),
            inputs,
            outputs,
            default: false,
            default_input: false,
        }
    }

    #[test]
    fn the_first_choice_takes_the_default_input_and_not_a_loopback_of_the_output() {
        // Sorted by id, as `inventory` leaves them: the loopbacks come before the default input.
        let mut list = vec![
            endpoint("a-out-1-2", "Out 1-2", 0, 2),
            endpoint("b-loopback", "Loopback", 2, 0),
            endpoint("c-loopback-mix", "Loopback Mix", 2, 0),
            endpoint("d-in-1-2", "In 1-2", 2, 0),
            endpoint("e-in-3-4", "In 3-4", 2, 0),
        ];
        list[0].default = true;
        list[3].default_input = true;
        let first = &choices(&list)[0];
        assert_eq!(first.output.as_deref(), Some("a-out-1-2"));
        assert_eq!(first.input.as_deref(), Some("d-in-1-2"));
        // With no default input at all, a loopback still comes after the real inputs.
        list[3].default_input = false;
        let first = &choices(&list)[0];
        assert_eq!(first.input.as_deref(), Some("d-in-1-2"));
    }

    #[test]
    fn latency_uses_each_reported_component_at_the_reported_rate() {
        let l = Latency {
            rate: 96000,
            input: 480,
            output: 288,
            conversion: 96,
        };
        assert_eq!(l.parts_ms(), [5.0, 3.0, 1.0]);
        assert_eq!(l.total_ms(), 9.0);
    }
}
