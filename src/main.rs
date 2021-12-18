use anyhow::Context;
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use rustsynth::define_input;
use rustsynth::define_rack;
use rustsynth::input::{ButtonMode, FieldType, Key};
use rustsynth::input::{InputConfig, OutputConfig, StateInput, StateOutput};
use rustsynth::midi_message::MidiMessage;
use rustsynth::module::{Buf, Rack, EG, IIRLPF, VCO};
use rustsynth::util::SyncError;
use rustsynth::WaveForm;

define_input! {
    Input {
        vco1_freq: f32 = 0.5,
        vco1_waveform: WaveForm = (WaveForm::Sine),
        lfo1_freq: f32 = 0.5,
        lfo1_waveform: WaveForm = (WaveForm::Sine),
        vco1_lfo1_amount: f32 = 0.0,
        eg1_a: f32 = 0.1,
        eg1_d: f32 = 0.05,
        eg1_s: f32 = 0.8,
        eg1_r: f32 = 0.1,
        eg1_gate: bool = false,
        eg1_repeat: bool = false,
        lpf1_freq: f32 = 0.1,
        lpf1_resonance: f32 = 0.05,
        lpf1_lfo1_amount: f32 = 0.0,
    }
}

#[derive(Debug)]
struct Config {
    midi_in_name: Option<String>,
    midi_out_name: Option<String>,
    rack_name: String,
    keys: toml::map::Map<String, toml::value::Value>,
}
fn load_config(path: &str) -> Result<Config> {
    use std::io::Read;
    use toml::Value;

    let mut file = std::fs::File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let parsed = content.parse::<Value>()?;
    let midi_in_name = parsed.get("default").and_then(|d| d.get("input"));
    let midi_in_name = match midi_in_name {
        Some(x) => Some(
            x.as_str()
                .context("Type error at default.input")?
                .to_owned(),
        ),
        None => None,
    };
    let midi_out_name = parsed.get("default").and_then(|d| d.get("output"));
    let midi_out_name = match midi_out_name {
        Some(x) => Some(
            x.as_str()
                .context("Type error at default.output")?
                .to_owned(),
        ),
        None => None,
    };
    let rack_name = parsed
        .get("rack")
        .and_then(|d| d.get("name"))
        .context("rack.name is not defined")?
        .as_str()
        .context("Type error at rack.name")?
        .to_owned();
    let keys = parsed
        .get("keys")
        .and_then(|d| d.as_table())
        .map(|d| d.clone())
        .unwrap_or_else(|| toml::map::Map::new());
    Ok(Config {
        midi_in_name,
        midi_out_name,
        rack_name,
        keys,
    })
}

fn build_state_io_from_config<S>(
    config: &Config,
    state_in: &mut StateInput<S>,
    state_out: &mut StateOutput<S>,
) -> Result<()> {
    for (name, value) in config.keys.iter() {
        match state_in.field_type(name) {
            None => {
                anyhow::bail!("Field not defined: {}", name);
            }
            Some(FieldType::F32) => {
                let key = value
                    .as_integer()
                    .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}", name))?;
                let key = Key::ControlChange(key as u8);
                state_in.define_input(
                    key,
                    InputConfig::F32 {
                        name: name.to_owned(),
                    },
                )
            }
            Some(FieldType::Bool) => {
                let value = value
                    .as_table()
                    .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}", name))?;
                let key = match value.get("key") {
                    None => None,
                    Some(x) => Some(
                        x.as_integer()
                            .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}.key", name))?,
                    ),
                };
                let key = key.map(|x| Key::ControlChange(x as u8));
                if let Some(key) = key {
                    let mode = match value.get("mode") {
                        Some(toml::value::Value::String(s)) => match s.as_ref() {
                            "toggle" => ButtonMode::Toggle,
                            "momentary" => ButtonMode::Momentary,
                            _ => return Err(anyhow::anyhow!("Invalid mode at keys.{}.mode", name)),
                        },
                        Some(_) => return Err(anyhow::anyhow!("Type error at keys.{}.mode", name)),
                        None => return Err(anyhow::anyhow!("keys.{}.mode required", name)),
                    };
                    state_in.define_input(
                        key,
                        InputConfig::Bool {
                            name: name.to_owned(),
                            mode,
                        },
                    );
                }
                let out = match value.get("out") {
                    Some(toml::value::Value::Integer(n)) => Some(Key::ControlChange(*n as u8)),
                    Some(_) => return Err(anyhow::anyhow!("Type error at keys.{}.out", name)),
                    None => None,
                };
                if let Some(out) = out {
                    state_out.define_output(OutputConfig::Bool {
                        name: name.to_owned(),
                        out,
                    });
                }
            }
            Some(FieldType::Enum) => {
                let value = value
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}", name))?;
                for (i, v) in value.iter().enumerate() {
                    let v = v
                        .as_table()
                        .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}[{}]", name, i))?;
                    let key = v.get("key");
                    let key = match key.map(|x| x.as_integer()) {
                        Some(Some(v)) => Some(Key::ControlChange(v as u8)),
                        Some(None) => {
                            return Err(anyhow::anyhow!("Type error at keys.{}[{}].key", name, i))
                        }
                        None => None,
                    };
                    let out = v.get("out");
                    let out = match out.map(|x| x.as_integer()) {
                        Some(Some(v)) => Some(Key::ControlChange(v as u8)),
                        Some(None) => {
                            return Err(anyhow::anyhow!("Type error at keys.{}[{}].out", name, i))
                        }
                        None => None,
                    };
                    let values = v
                        .get("values")
                        .ok_or_else(|| anyhow::anyhow!("Required: keys.{}[{}].values", name, i))?
                        .as_array()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Type error at keys.{}[{}].values", name, i)
                        })?;
                    let values = values
                        .iter()
                        .map(|x| x.as_str().map(|x| x.to_owned()))
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Type error at keys.{}.[{}].values", name, i)
                        })?;
                    if let Some(key) = key {
                        state_in.define_input(
                            key,
                            InputConfig::Enum {
                                name: name.to_owned(),
                                values: values.clone(),
                            },
                        );
                    }
                    if let Some(out) = out {
                        state_out.define_output(OutputConfig::Enum {
                            name: name.to_owned(),
                            values,
                            out,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

define_rack! {
    MyRack: Rack<Input>(rack, input) {
        lfo1: VCO {
            in_freq: {input.lfo1_freq },
            in_waveform: { input.lfo1_waveform } ,
            freq_min: 0.1,
            freq_max: 100.0,
        },
        vco1: VCO {
            in_freq: { rack.lfo1.borrow().out * input.vco1_lfo1_amount + input.vco1_freq } ,
            in_waveform: { input.vco1_waveform } ,
            freq_min: 100.0,
            freq_max: 15000.0,
        },
        eg1: EG {
            in_gate: { input.eg1_gate },
            in_repeat: { input.eg1_repeat },
            in_a: { input.eg1_a },
            in_d: { input.eg1_d },
            in_s: { input.eg1_s },
            in_r: { input.eg1_r },
        },
        vca1: Buf {
            in_value: { rack.vco1.borrow().out * rack.eg1.borrow().out },
        },
        lpf1: IIRLPF {
            in_freq: { input.lpf1_freq + input.lpf1_lfo1_amount * rack.lfo1.borrow().out },
            in_resonance: { input.lpf1_resonance },
            in_value: { rack.vca1.borrow().out },
            freq_min: 100.0,
            freq_max: 20_000.0,
        },
    }
}

fn rack_output(rack: &MyRack) -> f32 {
    rack.lpf1.borrow().out
}

fn main() -> Result<()> {
    let midi_out_con = setup_midi_output_connection()?;
    let midi_in = setup_midi_input()?;

    let cpal_device = setup_cpal_device()?;
    let cpal_config = setup_cpal_config(&cpal_device)?;

    run_synth(midi_in, midi_out_con, cpal_device, cpal_config)?;
    Ok(())
}

fn list_available_midi_ports<T: midir::MidiIO>(io: &T, kind: &str) -> Result<()> {
    println!("Available {} ports:", kind);
    for port in io.ports() {
        println!("* {}", io.port_name(&port)?);
    }
    Ok(())
}

fn setup_midi_output_connection() -> Result<midir::MidiOutputConnection> {
    let output = midir::MidiOutput::new("midir")?;
    list_available_midi_ports(&output, "output")?;
    let port = &output.ports()[0];
    let port_name = output.port_name(port)?;
    let out_con = output.connect(port, &port_name).map_err(SyncError::new)?;
    println!("Using device {}", port_name);
    Ok(out_con)
}

fn setup_midi_input() -> Result<midir::MidiInput> {
    let mut input = midir::MidiInput::new("midi_input")?;
    list_available_midi_ports(&input, "input")?;
    input.ignore(midir::Ignore::None);
    Ok(input)
}

fn setup_cpal_device() -> Result<cpal::Device> {
    let host = cpal::default_host();
    println!("Avaliable devices:");
    for device in host.output_devices()? {
        println!("* {}", device.name()?);
    }

    let device = host
        .default_output_device()
        .context("Default output device not found")?;
    println!("Using device {}", device.name()?);
    Ok(device)
}

fn setup_cpal_config(cpal_device: &cpal::Device) -> Result<cpal::StreamConfig> {
    println!("Available output config:");
    for config in cpal_device.supported_output_configs()? {
        println!("* {:?}", config);
    }
    let output_available = cpal_device.supported_output_configs()?.any(|c| {
        c.sample_format() == cpal::SampleFormat::F32
            && c.channels() == 2
            && c.min_sample_rate() <= cpal::SampleRate(44_100)
            && c.max_sample_rate() >= cpal::SampleRate(44_100)
            && match c.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } => min <= &441 && &441 <= max,
                _ => false,
            }
    });
    if !output_available {
        panic!("No suitable output available")
    }
    let cpal_config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(44_100),
        buffer_size: cpal::BufferSize::Fixed(441),
    };
    Ok(cpal_config)
}

fn set_led(midi_out: &mut midir::MidiOutputConnection, num: u8, on: bool) -> Result<()> {
    if on {
        midi_out.send(&[0xB0, num, 0x7F])?;
    } else {
        midi_out.send(&[0xB0, num, 0x00])?;
    }
    Ok(())
}

fn output<S>(
    state_out: &StateOutput<S>,
    state: &S,
    midi_out: &mut midir::MidiOutputConnection,
) -> Result<()> {
    state_out.output(state, |key, on| match key {
        Key::ControlChange(num) => set_led(midi_out, *num, on),
    })
}

fn run_synth(
    midi_in: midir::MidiInput,
    mut midi_out: midir::MidiOutputConnection,
    device: cpal::Device,
    stream_config: cpal::StreamConfig,
) -> Result<()> {
    let input = std::sync::Arc::new(std::sync::Mutex::new(Input {
        ..Default::default()
    }));
    let port = &midi_in.ports()[0];
    let port_name = midi_in.port_name(port)?;
    println!("Connect to {}", &port_name);
    let state_definition = Input::new_state_definition();
    let config = load_config("nanokontrol2.toml")?;
    let (mut state_in, mut state_out) = state_definition.into_io();
    build_state_io_from_config(&config, &mut state_in, &mut state_out)?;
    dbg!(&state_in);
    dbg!(&state_out);
    // setup_state_io(&mut state_in, &mut state_out)?;
    output(&state_out, &*input.lock().unwrap(), &mut midi_out)?;
    let _in_con = midi_in
        .connect(
            port,
            &port_name,
            {
                let input = std::sync::Arc::clone(&input);
                move |stamp, message, _| {
                    print!("{:10}", stamp);
                    let message = MidiMessage::try_from(message);
                    match message {
                        Ok(message) => {
                            println!("Message: {:0X?}", message);
                            let input = {
                                let mut input = input.lock().unwrap();
                                if let MidiMessage::ControlChange { ch: 0, num, value } = message {
                                    state_in.update_state(
                                        &mut input,
                                        Key::ControlChange(num),
                                        value,
                                    );
                                }
                                input.clone()
                            };
                            output(&state_out, &input, &mut midi_out).expect("LED update failed");
                        }
                        Err(err) => println!("Error: {:?}", err),
                    };
                }
            },
            (),
        )
        .map_err(SyncError::new)?;

    let rack = MyRack::new();
    let stream = device.build_output_stream(
        &stream_config,
        {
            let input = std::sync::Arc::clone(&input);
            move |data: &mut [f32], _| {
                let input = input.lock().unwrap();
                let input = &*input;
                for frame in data.chunks_mut(2) {
                    rack.update(input);
                    let value = rack_output(&rack);
                    for sample in frame.iter_mut() {
                        *sample = value;
                    }
                }
            }
        },
        |err| {
            println!("Device output error: {}", err);
        },
    )?;
    stream.play()?;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(2000));
        let _lock = dbg!(input.lock());
    }
}
