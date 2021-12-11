use anyhow::Context;
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustsynth::midi_message::MidiMessage;

fn main() -> Result<()> {
    run_synth()?;
    Ok(())
}

#[derive(Debug)]
struct SyncError<T: std::error::Error>(std::sync::Mutex<T>);
impl<T: std::error::Error> SyncError<T> {
    fn new(value: T) -> SyncError<T> {
        SyncError(std::sync::Mutex::new(value))
    }
}
impl<T: std::error::Error + std::fmt::Display> std::fmt::Display for SyncError<T> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self.0.lock() {
            Ok(value) => {
                value.fmt(fmt)?;
            }
            Err(err) => {
                panic!("Can't acquire lock: {}", err);
            }
        }
        Ok(())
    }
}
impl<T: std::error::Error> std::error::Error for SyncError<T> {}

fn list_available_ports<T: midir::MidiIO>(io: &T, kind: &str) -> Result<()> {
    println!("Available {} ports:", kind);
    for port in io.ports() {
        println!("* {}", io.port_name(&port)?);
    }
    Ok(())
}

fn run_synth() -> Result<()> {
    let output = midir::MidiOutput::new("midir")?;
    list_available_ports(&output, "output")?;
    let port = &output.ports()[0];
    let port_name = output.port_name(port)?;
    let out_con = output.connect(port, &port_name).map_err(SyncError::new)?;
    println!("Using device {}", port_name);

    let mut input = midir::MidiInput::new("midi_input")?;
    list_available_ports(&input, "input")?;
    input.ignore(midir::Ignore::None);
    run_synth_main(input, out_con)?;
    Ok(())
}

fn update_input(input: &mut rustsynth::Input, message: &MidiMessage) {
    use rustsynth::WaveForm;
    if let MidiMessage::ControlChange { ch: 0, num, value } = message {
        let value = *value as f32 / 127.0;
        match num {
            0x00 => input.lfo1_freq = value,
            0x01 => input.vco1_freq = value,
            0x10 => input.vco1_lfo1_amount = value,
            _ => {
                if value > 0.5 {
                    match num {
                        0x20 => {
                            if input.lfo1_waveform == WaveForm::Sine {
                                input.lfo1_waveform = WaveForm::Triangle;
                            } else {
                                input.lfo1_waveform = WaveForm::Sine;
                            }
                        }
                        0x30 => input.lfo1_waveform = WaveForm::Sawtooth,
                        0x40 => input.lfo1_waveform = WaveForm::Square,
                        0x21 => {
                            if input.vco1_waveform == WaveForm::Sine {
                                input.vco1_waveform = WaveForm::Triangle;
                            } else {
                                input.vco1_waveform = WaveForm::Sine
                            }
                        }
                        0x31 => input.vco1_waveform = WaveForm::Sawtooth,
                        0x41 => input.vco1_waveform = WaveForm::Square,
                        _ => {}
                    }
                }
            }
        }
    }
}

fn set_led(midi_out: &mut midir::MidiOutputConnection, num: u8, on: bool) -> Result<()> {
    if on {
        midi_out.send(&[0xB0, num, 0x7F])?;
    } else {
        midi_out.send(&[0xB0, num, 0x00])?;
    }
    Ok(())
}

fn update_led(input: &rustsynth::Input, midi_out: &mut midir::MidiOutputConnection) -> Result<()> {
    use rustsynth::WaveForm;
    let led_group_0 = [0x20, 0x30, 0x40];
    let led_group_0_lit = match input.lfo1_waveform {
        WaveForm::Sine | WaveForm::Triangle => 0x20,
        WaveForm::Sawtooth => 0x30,
        WaveForm::Square => 0x40,
    };
    for led in led_group_0 {
        set_led(midi_out, led, led == led_group_0_lit)?;
    }

    let led_group_1 = [0x21, 0x31, 0x41];
    let led_group_1_lit = match input.vco1_waveform {
        WaveForm::Sine | WaveForm::Triangle => 0x21,
        WaveForm::Sawtooth => 0x31,
        WaveForm::Square => 0x41,
    };
    for led in led_group_1 {
        set_led(midi_out, led, led == led_group_1_lit)?;
    }

    Ok(())
}

fn run_synth_main(
    midi_in: midir::MidiInput,
    mut midi_out: midir::MidiOutputConnection,
) -> Result<()> {
    let input = std::sync::Arc::new(std::sync::Mutex::new(rustsynth::Input {
        ..Default::default()
    }));
    update_led(&*input.lock().unwrap(), &mut midi_out)?;
    let port = &midi_in.ports()[0];
    let port_name = midi_in.port_name(port)?;
    println!("Connect to {}", &port_name);
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
                                update_input(std::ops::DerefMut::deref_mut(&mut input), &message);
                                (&*input).clone()
                            };
                            update_led(&input, &mut midi_out).expect("update_led failed")
                        }
                        Err(err) => println!("Error: {:?}", err),
                    };
                }
            },
            (),
        )
        .map_err(SyncError::new)?;

    let host = cpal::default_host();
    println!("Avaliable devices:");
    for device in host.output_devices()? {
        println!("* {}", device.name()?);
    }

    let device = host
        .default_output_device()
        .context("Default output device not found")?;
    println!("Using device {}", device.name()?);

    println!("Available output config:");
    for config in device.supported_output_configs()? {
        println!("* {:?}", config);
    }
    let output_available = device.supported_output_configs()?.any(|c| {
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
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(44_100),
        buffer_size: cpal::BufferSize::Fixed(441),
    };
    let rack = rustsynth::MyRack::new();
    let stream = device.build_output_stream(
        &config,
        {
            let input = std::sync::Arc::clone(&input);
            move |data: &mut [f32], _| {
                let input = input.lock().unwrap();
                let input = &*input;
                for frame in data.chunks_mut(2) {
                    rack.update(input);
                    let value = rack.vco1.borrow().out;
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
        let _ = dbg!(input.lock());
    }
}
