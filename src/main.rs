use anyhow::Context;
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use rustsynth::define_input;
use rustsynth::define_rack;
use rustsynth::input::Key;
use rustsynth::input::StateOutput;
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
    let config = rustsynth::config::load_config("nanokontrol2.toml")?;
    let (mut state_in, mut state_out) = state_definition.into_io();
    rustsynth::config::setup_state_io(&config, &mut state_in, &mut state_out)?;
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
