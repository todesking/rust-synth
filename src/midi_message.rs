pub enum MidiMessage {
    Unknown(Vec<u8>),
    ControlChange { ch: u8, num: u8, value: u8 },
    SysEx(Vec<u8>),
}

#[derive(Debug)]
pub struct MidiMessageParseError {}
fn get_at(value: &[u8], index: usize) -> std::result::Result<u8, MidiMessageParseError> {
    if index < value.len() {
        Ok(value[index])
    } else {
        Err(MidiMessageParseError {})
    }
}
impl std::convert::TryFrom<&[u8]> for MidiMessage {
    type Error = MidiMessageParseError;
    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        let status = get_at(value, 0)?;
        let kind = status & 0xF0;
        let ch = status & 0x0F;
        match kind {
            #[allow(clippy::if_same_then_else)]
            0xB0 => {
                let control = get_at(value, 1)?;
                if control <= 0x77 {
                    // control change
                    let control_value = get_at(value, 2)?;
                    Ok(MidiMessage::ControlChange {
                        ch,
                        num: control,
                        value: control_value,
                    })
                } else if control <= 0x7F {
                    // channel message
                    Ok(MidiMessage::Unknown(value.to_vec()))
                } else {
                    // ???
                    Ok(MidiMessage::Unknown(value.to_vec()))
                }
            }
            0xF0 => {
                if value[value.len() - 1] == 0xF7 {
                    Ok(MidiMessage::SysEx(value[1..value.len() - 1].to_vec()))
                } else {
                    Ok(MidiMessage::SysEx(value[1..].to_vec()))
                }
            }
            _ => Ok(MidiMessage::Unknown(value.to_vec())),
        }
    }
}
impl std::fmt::Debug for MidiMessage {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            MidiMessage::Unknown(value) => fmt.write_fmt(format_args!("Unknown({:02X?})", value)),
            MidiMessage::SysEx(value) => fmt.write_fmt(format_args!("SysEx({:02X?})", value)),
            MidiMessage::ControlChange { ch, num, value } => fmt
                .debug_struct("ControlChange")
                .field("ch", ch)
                .field("num", num)
                .field("value", value)
                .finish(),
        }
    }
}
