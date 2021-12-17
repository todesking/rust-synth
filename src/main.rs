use anyhow::Context;
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use rustsynth::define_rack;
use rustsynth::midi_message::MidiMessage;
use rustsynth::util::SyncError;
use rustsynth::WaveForm;
use rustsynth::{Buf, Rack, EG, IIRLPF, VCO};

pub trait SimpleEnum
where
    Self: Sized,
{
    fn from_name(name: &str) -> Option<Self>;
    fn to_name(&self) -> &'static str;
}
impl SimpleEnum for WaveForm {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "Sine" => Some(WaveForm::Sine),
            "Triangle" => Some(WaveForm::Triangle),
            "Sawtooth" => Some(WaveForm::Sawtooth),
            "Square" => Some(WaveForm::Square),
            "Noise" => Some(WaveForm::Noise),
            _ => None,
        }
    }
    fn to_name(&self) -> &'static str {
        match self {
            WaveForm::Sine => "Sine",
            WaveForm::Triangle => "Triangle",
            WaveForm::Sawtooth => "Sawtooth",
            WaveForm::Square => "Square",
            WaveForm::Noise => "Noise",
        }
    }
}

pub enum ButtonMode {
    Toggle,
    Momentary,
}

pub enum InputConfig {
    F32 { name: String },
    Bool { name: String, mode: ButtonMode },
    Enum { name: String, values: Vec<String> },
}
impl InputConfig {
    fn name(&self) -> &str {
        match self {
            Self::F32 { name } => name,
            Self::Bool { name, .. } => name,
            Self::Enum { name, .. } => name,
        }
    }
}

pub enum OutputConfig {
    Bool {
        name: String,
        out: Key,
    },
    Enum {
        name: String,
        out: Key,
        values: Vec<String>,
    },
}
impl OutputConfig {
    fn name(&self) -> &str {
        match self {
            OutputConfig::Bool { name, .. } => name,
            OutputConfig::Enum { name, .. } => name,
        }
    }
}

pub struct StateDefinition<S> {
    accessors: std::collections::HashMap<String, FieldAccessor<S>>,
}
pub struct StateInput<S> {
    state_definition: std::sync::Arc<StateDefinition<S>>,
    inputs: std::collections::HashMap<Key, InputConfig>,
}
pub struct StateOutput<S> {
    state_definition: std::sync::Arc<StateDefinition<S>>,
    outputs: Vec<OutputConfig>,
}
impl<S> StateDefinition<S> {
    pub fn new() -> Self {
        Self {
            accessors: std::collections::HashMap::new(),
        }
    }
    pub fn into_io(self) -> (StateInput<S>, StateOutput<S>) {
        let sd = std::sync::Arc::new(self);
        (StateInput::new(sd.clone()), StateOutput::new(sd.clone()))
    }
    pub fn define_field(&mut self, name: String, accessor: FieldAccessor<S>) {
        self.accessors.insert(name, accessor);
    }
    pub fn assert_has_field(&self, name: &str) {
        if !self.accessors.contains_key(name) {
            panic!("Undefined field: {}", name);
        }
    }
    pub fn field(&self, name: &str) -> &FieldAccessor<S> {
        self.accessors
            .get(name)
            .unwrap_or_else(|| panic!("Undefined field: {}", name))
    }
}
impl<S> StateInput<S> {
    pub fn new(state_definition: std::sync::Arc<StateDefinition<S>>) -> StateInput<S> {
        StateInput {
            state_definition,
            inputs: std::collections::HashMap::new(),
        }
    }
    pub fn define_input(&mut self, key: Key, input: InputConfig) {
        self.state_definition.assert_has_field(input.name());
        self.inputs.insert(key, input);
    }
    pub fn update_state(&self, state: &mut S, key: Key, value: u8) {
        if let Some(input) = self.inputs.get(&key) {
            match input {
                InputConfig::Bool { name, mode } => match self.state_definition.field(name) {
                    FieldAccessor::Bool(get, set) => {
                        let pressed = 0x40 <= value;
                        match mode {
                            ButtonMode::Momentary => {
                                set(state, pressed);
                            }
                            ButtonMode::Toggle => {
                                let current = get(state);
                                if pressed {
                                    set(state, !current);
                                }
                            }
                        }
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
                InputConfig::F32 { name } => match self.state_definition.field(name) {
                    FieldAccessor::F32(_, set) => {
                        let value = value as f32 / 127.0f32;
                        set(state, value);
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
                InputConfig::Enum { name, values } => match self.state_definition.field(name) {
                    FieldAccessor::Enum(get, set) => {
                        let pressed = 0x40 <= value;
                        if pressed {
                            let current = get(state);
                            let mut index = 0;
                            for (i, v) in values.iter().enumerate() {
                                if v == current {
                                    index = (i + 1) % values.len();
                                    break;
                                }
                            }
                            set(state, &values[index]);
                        }
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
            }
        }
    }
}
impl<S> StateOutput<S> {
    pub fn new(state_definition: std::sync::Arc<StateDefinition<S>>) -> StateOutput<S> {
        StateOutput {
            state_definition,
            outputs: Vec::new(),
        }
    }
    pub fn define_output(&mut self, output: OutputConfig) {
        self.state_definition.assert_has_field(output.name());
        self.outputs.push(output);
    }
    pub fn output<F: FnMut(&Key, bool) -> Result<()>>(&self, state: &S, mut f: F) -> Result<()> {
        for o in self.outputs.iter() {
            match o {
                OutputConfig::Bool { name, out } => match self.state_definition.field(name) {
                    FieldAccessor::Bool(get, _) => {
                        f(out, get(state))?;
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
                OutputConfig::Enum { name, out, values } => {
                    match self.state_definition.field(name) {
                        FieldAccessor::Enum(get, _) => {
                            let s = get(state);
                            f(out, values.iter().any(|v| v == s))?;
                        }
                        _ => {
                            panic!("assertion error: {}", name);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

type Get<S, T> = Box<dyn Fn(&S) -> T + Send + Sync>;
type Set<S, T> = Box<dyn Fn(&mut S, T) + Send + Sync>;
pub enum FieldAccessor<S> {
    F32(Get<S, f32>, Set<S, f32>),
    Bool(Get<S, bool>, Set<S, bool>),
    Enum(
        Get<S, &'static str>,
        Box<dyn Fn(&mut S, &str) + Send + Sync>,
    ),
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum Key {
    ControlChange(u8),
}

pub trait DefineField<S, T> {
    fn define_field<
        Get: Fn(&S) -> &T + 'static + Send + Sync,
        Set: Fn(&mut S, T) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    );
}
impl<S> DefineField<S, f32> for StateDefinition<S> {
    fn define_field<
        Get: Fn(&S) -> &f32 + 'static + Send + Sync,
        Set: Fn(&mut S, f32) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    ) {
        self.define_field(
            name,
            FieldAccessor::F32(
                Box::new(move |input| *get(input)),
                Box::new(move |input, value| set(input, value)),
            ),
        )
    }
}
impl<S> DefineField<S, bool> for StateDefinition<S> {
    fn define_field<
        Get: Fn(&S) -> &bool + 'static + Send + Sync,
        Set: Fn(&mut S, bool) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    ) {
        self.define_field(
            name,
            FieldAccessor::Bool(
                Box::new(move |input| *get(input)),
                Box::new(move |input, value| set(input, value)),
            ),
        )
    }
}
impl<S, E: SimpleEnum> DefineField<S, E> for StateDefinition<S> {
    fn define_field<
        Get: Fn(&S) -> &E + 'static + Send + Sync,
        Set: Fn(&mut S, E) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    ) {
        self.define_field(
            name,
            FieldAccessor::Enum(
                Box::new(move |input| get(input).to_name()),
                Box::new(move |input, value| {
                    if let Some(value) = E::from_name(value) {
                        set(input, value)
                    }
                }),
            ),
        )
    }
}

macro_rules! define_input_field_default {
    () => {
        ::std::default::Default::default()
    };
    ($expr:expr) => {
        $expr
    };
}
macro_rules! define_input {
    ($name:ident {
        $($field:ident : $ty:ty $(= $default_value:tt)?),*$(,)?
    }) => {
        #[derive(Clone, Debug)]
        pub struct $name {
            $(
                pub $field: $ty
            ),*
        }
        impl $name {
            pub fn new_state_definition() -> StateDefinition<Self> {
                let mut key_mapping = StateDefinition::<$name>::new();
                $(
                    DefineField::<$name, $ty>::define_field(
                        &mut key_mapping,
                        stringify!($field).to_owned(),
                        |input| &input.$field,
                        |input, value| input.$field = value
                    );
                )*
                key_mapping
            }
        }
        impl ::std::default::Default for $name {
            fn default() -> $name {
                $name {
                    $(
                        $field: define_input_field_default!($($default_value)?)
                    ),*
                }
            }
        }
    };
}

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
fn setup_input<S>(state_in: &mut StateInput<S>) {
    state_in.define_input(
        Key::ControlChange(0x00),
        InputConfig::F32 {
            name: "lfo1_freq".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x01),
        InputConfig::F32 {
            name: "vco1_freq".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x10),
        InputConfig::F32 {
            name: "vco1_lfo1_amount".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x02),
        InputConfig::F32 {
            name: "eg1_a".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x12),
        InputConfig::F32 {
            name: "eg1_d".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x03),
        InputConfig::F32 {
            name: "eg1_s".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x13),
        InputConfig::F32 {
            name: "eg1_r".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x04),
        InputConfig::F32 {
            name: "lpf1_freq".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x14),
        InputConfig::F32 {
            name: "lpf1_resonance".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x15),
        InputConfig::F32 {
            name: "lpf1_lfo1_amount".to_owned(),
        },
    );
    state_in.define_input(
        Key::ControlChange(0x20),
        InputConfig::Enum {
            name: "lfo1_waveform".to_owned(),
            values: vec!["Sine".to_owned(), "Triangle".to_owned()],
        },
    );
    state_in.define_input(
        Key::ControlChange(0x30),
        InputConfig::Enum {
            name: "lfo1_waveform".to_owned(),
            values: vec!["Sawtooth".to_owned()],
        },
    );
    state_in.define_input(
        Key::ControlChange(0x40),
        InputConfig::Enum {
            name: "lfo1_waveform".to_owned(),
            values: vec!["Square".to_owned(), "Noise".to_owned()],
        },
    );
    state_in.define_input(
        Key::ControlChange(0x21),
        InputConfig::Enum {
            name: "vco1_waveform".to_owned(),
            values: vec!["Sine".to_owned(), "Triangle".to_owned()],
        },
    );
    state_in.define_input(
        Key::ControlChange(0x31),
        InputConfig::Enum {
            name: "vco1_waveform".to_owned(),
            values: vec!["Sawtooth".to_owned()],
        },
    );
    state_in.define_input(
        Key::ControlChange(0x41),
        InputConfig::Enum {
            name: "vco1_waveform".to_owned(),
            values: vec!["Square".to_owned(), "Noise".to_owned()],
        },
    );
    state_in.define_input(
        Key::ControlChange(0x32),
        InputConfig::Bool {
            name: "eg1_repeat".to_owned(),
            mode: ButtonMode::Toggle,
        },
    );
    state_in.define_input(
        Key::ControlChange(0x42),
        InputConfig::Bool {
            name: "eg1_gate".to_owned(),
            mode: ButtonMode::Momentary,
        },
    );
}
fn setup_output<S>(state_out: &mut StateOutput<S>) {
    state_out.define_output(OutputConfig::Enum {
        name: "lfo1_waveform".to_owned(),
        values: vec!["Sine".to_owned(), "Triangle".to_owned()],
        out: Key::ControlChange(0x20),
    });
    state_out.define_output(OutputConfig::Enum {
        name: "lfo1_waveform".to_owned(),
        values: vec!["Sawtooth".to_owned()],
        out: Key::ControlChange(0x30),
    });
    state_out.define_output(OutputConfig::Enum {
        name: "lfo1_waveform".to_owned(),
        values: vec!["Square".to_owned(), "Noise".to_owned()],
        out: Key::ControlChange(0x40),
    });
    state_out.define_output(OutputConfig::Enum {
        name: "vco1_waveform".to_owned(),
        values: vec!["Sine".to_owned(), "Triangle".to_owned()],
        out: Key::ControlChange(0x21),
    });
    state_out.define_output(OutputConfig::Enum {
        name: "vco1_waveform".to_owned(),
        values: vec!["Sawtooth".to_owned()],
        out: Key::ControlChange(0x31),
    });
    state_out.define_output(OutputConfig::Enum {
        name: "vco1_waveform".to_owned(),
        values: vec!["Square".to_owned(), "Noise".to_owned()],
        out: Key::ControlChange(0x41),
    });
    state_out.define_output(OutputConfig::Bool {
        name: "eg1_gate".to_owned(),
        out: Key::ControlChange(0x42),
    });
    state_out.define_output(OutputConfig::Bool {
        name: "eg1_repeat".to_owned(),
        out: Key::ControlChange(0x32),
    });
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
    state_out.output(&state, |key, on| match key {
        Key::ControlChange(num) => set_led(midi_out, *num, on),
    })
}

fn run_synth(
    midi_in: midir::MidiInput,
    mut midi_out: midir::MidiOutputConnection,
    device: cpal::Device,
    config: cpal::StreamConfig,
) -> Result<()> {
    let input = std::sync::Arc::new(std::sync::Mutex::new(Input {
        ..Default::default()
    }));
    let port = &midi_in.ports()[0];
    let port_name = midi_in.port_name(port)?;
    println!("Connect to {}", &port_name);
    let state_definition = Input::new_state_definition();
    let (mut state_in, mut state_out) = state_definition.into_io();
    setup_input(&mut state_in);
    setup_output(&mut state_out);
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
                                match message {
                                    MidiMessage::ControlChange { ch: 0, num, value } => {
                                        state_in.update_state(
                                            &mut input,
                                            Key::ControlChange(num),
                                            value,
                                        );
                                    }
                                    _ => {}
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
        &config,
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
