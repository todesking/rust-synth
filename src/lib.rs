pub mod midi_message;
pub mod nanokontrol2;
pub mod util;

use std::marker::PhantomData;

pub trait Rack {
    type Input;
}
pub trait Module<R: Rack> {
    fn update(&mut self, rack: &R, input: &R::Input);
}

const SAMPLES_PER_SEC: u32 = 44_100;

pub trait InPort<R: Rack, T>: Fn(&R, &R::Input) -> T + std::marker::Send {}
impl<R: Rack, T, F: Fn(&R, &R::Input) -> T + std::marker::Send> InPort<R, T> for F {}

pub fn restore_freq(min: f32, max: f32, input: f32) -> f32 {
    (min.ln() + input * (max.ln() - min.ln())).exp()
}

pub struct VCO<R: Rack> {
    pub _rack: PhantomData<R>,
    // range: 0.0 - 1.0 ( freq_min Hz - freq_max Hz )
    pub in_freq: Box<dyn InPort<R, f32>>,
    pub in_waveform: Box<dyn InPort<R, WaveForm>>,
    pub phase: f32,
    pub freq_min: f32,
    pub freq_max: f32,
    pub out: f32,
}
impl<R: Rack> Default for VCO<R> {
    fn default() -> Self {
        VCO {
            _rack: PhantomData,
            in_freq: Box::new(|_, _| 0.0),
            in_waveform: Box::new(|_, _| WaveForm::Sine),
            phase: 0.0,
            freq_min: 0.0,
            freq_max: 0.0,
            out: 0.0,
        }
    }
}
impl<R: Rack> Module<R> for VCO<R> {
    fn update(&mut self, rack: &R, input: &R::Input) {
        let in_freq = (self.in_freq)(rack, input);
        let pi: f32 = std::f32::consts::PI;
        let pi2: f32 = pi * 2.0;
        let pi12: f32 = pi / 2.0;
        let pi32: f32 = pi12 * 3.0;
        let freq = restore_freq(self.freq_min, self.freq_max, in_freq);
        self.phase += freq * pi2 / SAMPLES_PER_SEC as f32;
        self.phase %= pi2;
        let wf = (self.in_waveform)(rack, input);
        self.out = match wf {
            WaveForm::Sine => (self.phase).sin(),
            WaveForm::Sawtooth => {
                if self.phase < pi {
                    self.phase / pi
                } else {
                    (self.phase - pi) / pi - 1.0
                }
            }
            WaveForm::Triangle => {
                if self.phase < pi12 {
                    self.phase / pi12
                } else if self.phase < pi32 {
                    1.0 - (self.phase - pi12) / pi12
                } else {
                    (self.phase - pi32) / pi12 - 1.0
                }
            }
            WaveForm::Square => {
                if self.phase < pi {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

pub struct EG<R: Rack> {
    pub _rack: PhantomData<R>,
    pub in_gate: Box<dyn InPort<R, bool>>,
    pub in_repeat: Box<dyn InPort<R, bool>>,
    /// sec
    pub in_a: Box<dyn InPort<R, f32>>,
    /// sec
    pub in_d: Box<dyn InPort<R, f32>>,
    /// 0.0 - 1.0
    pub in_s: Box<dyn InPort<R, f32>>,
    /// sec
    pub in_r: Box<dyn InPort<R, f32>>,
    pub clock: f32,
    pub state: EGState,
    pub level: f32,
    /// 0.0 - 1.0
    pub out: f32,
}
pub enum EGState {
    Idle,
    A,
    D,
    S,
    R,
}
impl<R: Rack> Default for EG<R> {
    fn default() -> Self {
        EG {
            _rack: PhantomData,
            in_gate: Box::new(|_, _| false),
            in_repeat: Box::new(|_, _| false),
            in_a: Box::new(|_, _| 0.0),
            in_d: Box::new(|_, _| 0.0),
            in_s: Box::new(|_, _| 1.0),
            in_r: Box::new(|_, _| 0.0),
            state: EGState::Idle,
            clock: 0.0,
            level: 0.0,
            out: 0.0,
        }
    }
}
impl<R: Rack> Module<R> for EG<R> {
    fn update(&mut self, rack: &R, input: &R::Input) {
        let gate = (self.in_gate)(rack, input);
        let repeat = (self.in_repeat)(rack, input);
        let a = (self.in_a)(rack, input);
        let d = (self.in_d)(rack, input);
        let s = (self.in_s)(rack, input);
        let r = (self.in_r)(rack, input);
        match self.state {
            EGState::Idle => {
                if gate || repeat {
                    self.state = EGState::A;
                    self.clock = 0.0;
                }
            }
            EGState::A => {
                if !gate && !repeat {
                    self.state = EGState::R;
                    self.clock = 0.0;
                    self.level = self.out;
                } else if self.clock >= a {
                    self.state = EGState::D;
                    self.clock = 0.0;
                }
            }
            EGState::D => {
                if !gate && !repeat {
                    self.state = EGState::R;
                    self.clock = 0.0;
                    self.level = self.out;
                } else if self.clock >= d {
                    self.state = EGState::S;
                    self.clock = 0.0;
                }
            }
            EGState::S => {
                if !gate {
                    self.state = EGState::R;
                    self.clock = 0.0;
                    self.level = self.out;
                }
            }
            EGState::R => {
                if !gate && self.clock >= r {
                    self.state = EGState::Idle;
                    self.clock = 0.0;
                    self.level = 0.0;
                } else if gate {
                    self.state = EGState::A;
                    self.clock = 0.0;
                    self.level = self.out;
                }
            }
        }
        match self.state {
            EGState::Idle => {
                self.clock = 0.0;
                self.out = 0.0;
            }
            EGState::A => {
                if a > 0.0 {
                    self.out = self.level.max(1.0 / a * self.clock);
                }
            }
            EGState::D => {
                if d > 0.0 {
                    self.out = 1.0 - ((1.0 - s) / d * self.clock);
                }
            }
            EGState::S => {
                self.out = s;
            }
            EGState::R => {
                if r > 0.0 {
                    self.out = (0.0f32).max(self.level - self.clock * self.level / r);
                }
            }
        }
        self.clock += 1.0 / SAMPLES_PER_SEC as f32;
    }
}

pub struct IIRLPF<R: Rack> {
    pub _rack: PhantomData<R>,
    /// 0.0 - 1.0
    pub in_freq: Box<dyn InPort<R, f32>>,
    /// 0.0 - 1.0
    pub in_resonance: Box<dyn InPort<R, f32>>,
    pub in_value: Box<dyn InPort<R, f32>>,
    pub freq_min: f32,
    pub freq_max: f32,
    pub buf_a: Vec<f32>,
    pub i_a: usize,
    pub buf_b: Vec<f32>,
    pub i_b: usize,
    pub out: f32,
}
/// n=0 -> newest, n=1 -> z^-1, ..., n=buf.len()-1
fn buf_at(buf: &[f32], i: usize, n: usize) -> f32 {
    if n < i {
        buf[i - 1 - n]
    } else {
        buf[buf.len() + i - 1 - n]
    }
}

impl<R: Rack> Default for IIRLPF<R> {
    fn default() -> Self {
        IIRLPF {
            _rack: PhantomData,
            in_freq: Box::new(|_, _| 0.0),
            in_resonance: Box::new(|_, _| 0.0),
            in_value: Box::new(|_, _| 0.0),
            freq_min: 100.0,
            freq_max: 10000.0,
            buf_a: vec![],
            buf_b: vec![],
            i_a: 0,
            i_b: 0,
            out: 0.0,
        }
    }
}
impl<R: Rack> Module<R> for IIRLPF<R> {
    fn update(&mut self, rack: &R, input: &R::Input) {
        let in_freq = (self.in_freq)(rack, input);
        let in_resonance = (self.in_resonance)(rack, input);
        let in_value = (self.in_value)(rack, input);

        let freq = restore_freq(self.freq_min, self.freq_max, in_freq);

        let fc = freq / SAMPLES_PER_SEC as f32;
        let q = (0.5 + in_resonance * 9.5) / 2.0f32.sqrt();
        use std::f32::consts::PI;
        // reference: 青木直史. サウンドプログラミング入門. 技術評論社, 2018
        let a0 = 1.0 + 2.0 * PI * fc / q + 4.0 * PI * PI * fc * fc;
        let a = [
            1.0,
            (8.0 * PI * PI * fc * fc - 2.0) / a0,
            (1.0 - 2.0 * PI * fc / q + 4.0 * PI * PI * fc * fc) / a0,
        ];
        let b = [
            4.0 * PI * PI * fc * fc / a0,
            8.0 * PI * PI * fc * fc / a0,
            4.0 * PI * PI * fc * fc / a0,
        ];

        self.buf_a.resize(a.len(), 0.0);
        self.buf_b.resize(b.len(), 0.0);

        self.buf_b[self.i_b] = in_value;
        self.i_b += 1;
        self.i_b %= self.buf_b.len();

        let mut b_value = 0.0;
        for m in 0..=2 {
            b_value += b[m] * buf_at(&self.buf_b, self.i_b, m);
        }

        let mut a_value = b_value;
        for m in 1..=2 {
            a_value += -a[m] * buf_at(&self.buf_a, self.i_a, m - 1);
        }

        self.buf_a[self.i_a] = a_value;
        self.i_a += 1;
        self.i_a %= self.buf_a.len();

        self.out = a_value;
    }
}

pub struct Buf<R: Rack> {
    pub _rack: PhantomData<R>,
    pub in_value: Box<dyn InPort<R, f32>>,
    pub out: f32,
}
impl<R: Rack> Default for Buf<R> {
    fn default() -> Self {
        Buf {
            _rack: PhantomData,
            in_value: Box::new(|_, _| 0.0),
            out: 0.0,
        }
    }
}
impl<R: Rack> Module<R> for Buf<R> {
    fn update(&mut self, rack: &R, input: &<R as Rack>::Input) {
        self.out = (self.in_value)(rack, input);
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WaveForm {
    Sine,
    Sawtooth,
    Triangle,
    Square,
}
impl Default for WaveForm {
    fn default() -> WaveForm {
        WaveForm::Sine
    }
}

#[macro_export]
macro_rules! define_rack {
    ($rack_name:ident : Rack<$input:ident> {$(
        $mod_name:ident : $mod_type:ident {$(
            $field_name:ident : $field_value:expr
        ),*$(,)?}
    ),*$(,)?}) => {
        pub struct $rack_name {
            $(pub $mod_name: ::std::cell::RefCell<$mod_type<$rack_name>> ),*
        }
        impl $rack_name {
            pub fn new() -> $rack_name {
                $rack_name {
                    $($mod_name: ::std::cell::RefCell::new(
                        $mod_type {
                            $($field_name: $field_value),*
                            ,..::std::default::Default::default()
                        }
                    )),*
                }
            }
            pub fn update(&self, input: &$input) {
                $({
                    let mut module = ::std::cell::RefCell::borrow_mut(&self.$mod_name);
                    ::rustsynth::Module::update(&mut *module, self, input);
                })*
            }
        }
        impl ::rustsynth::Rack for $rack_name {
            type Input = $input;
        }
    };
}
