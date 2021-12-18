pub mod config;
pub mod input;
pub mod macros;
pub mod midi_message;
pub mod module;
pub mod nanokontrol2;
pub mod util;

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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WaveForm {
    Sine,
    Sawtooth,
    Triangle,
    Square,
    Noise,
}
impl Default for WaveForm {
    fn default() -> WaveForm {
        WaveForm::Sine
    }
}
