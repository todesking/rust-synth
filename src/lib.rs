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
        let freq = (self.freq_min.ln() + in_freq * (self.freq_max.ln() - self.freq_min.ln())).exp();
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
